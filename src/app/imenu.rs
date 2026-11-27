use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{Context, Entity, Window};

use super::AppShell;
use super::candidates::CompletionWork;
use super::documents::VisitOptions;
use crate::completion::{self, Candidate, CandidateList, MatchedCandidate};
use crate::input::DisplayId;
use crate::outline::{
    FlatSymbol, OpenFileOverlay, ProjectOutlineCache, ProjectSymbol, extract_from_rope,
    extract_from_tree, flatten, scan_project,
};
use crate::project::Project;
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "imenu_tests.rs"]
mod tests;

const EMPTY_IMENU_MESSAGE: &str = "No items suitable for an index found in this buffer";

impl AppShell {
    pub(super) fn collect_open_overlays(&self) -> HashMap<PathBuf, OpenFileOverlay> {
        let mut overlays = HashMap::new();
        for slot in self.buffers.iter() {
            let buffer = &slot.buffer;
            let Some(path) = buffer.path() else {
                continue;
            };
            let Some(language) = buffer.syntax_language() else {
                continue;
            };
            let tree = buffer.syntax_tree().map(|(_, _, tree)| tree);
            overlays.insert(
                path,
                OpenFileOverlay {
                    snapshot: buffer.snapshot(),
                    language,
                    tree,
                },
            );
        }
        overlays
    }

    pub(super) fn imenu_command(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        let buffer = origin.read(cx).buffer();
        let Some(language) = buffer.syntax_language() else {
            self.show_message(origin, EMPTY_IMENU_MESSAGE, cx);
            return;
        };
        if let Some((tree_lang, _, tree)) = buffer.syntax_tree() {
            let text = buffer.text();
            let symbols = extract_from_tree(tree_lang, &text, &tree, &AtomicBool::new(false))
                .map(|syms| flatten(&syms))
                .unwrap_or_default();
            if symbols.is_empty() {
                self.show_message(origin, EMPTY_IMENU_MESSAGE, cx);
                return;
            }
            let default = buffer.symbol_at_point().and_then(|sym| {
                symbols
                    .iter()
                    .find(|item| item.qualified_name == sym)
                    .or_else(|| symbols.iter().find(|item| item.name == sym))
                    .map(|item| item.qualified_name.clone())
            });
            if self
                .begin_read(
                    ReadKind::Imenu {
                        default,
                        ready: true,
                    },
                    origin,
                    "",
                    window,
                    cx,
                )
                .is_some()
                && let Some(mount) = &mut self.minibuffer
            {
                mount.imenu_symbols = Some(Arc::new(symbols));
                mount.candidate_snapshot = None;
                self.refresh_candidates(window, cx);
            }
            return;
        }
        let _ = language;
        self.begin_read(
            ReadKind::Imenu {
                default: None,
                ready: false,
            },
            origin,
            "",
            window,
            cx,
        );
    }

    pub(super) fn begin_project_imenu(
        &mut self,
        project: Project,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        self.begin_read(ReadKind::ProjectImenu { project }, origin, "", window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn refresh_imenu_candidates(
        &mut self,
        id: u64,
        default: Option<&str>,
        ready: bool,
        origin: &Entity<BufferView>,
        input: &str,
        snapshot: DisplayId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mount) = &mut self.minibuffer else {
            return;
        };
        if let Some(symbols) = mount.imenu_symbols.clone() {
            let items = filter_imenu_symbols(&symbols, input, &AtomicBool::new(false));
            self.session.as_mut().unwrap().candidates =
                Some(CandidateList::ready(items, input, default, false));
            self.publish_candidates(cx);
            return;
        }
        if ready || mount.imenu_work.is_some() {
            return;
        }
        let buffer = origin.read(cx).buffer();
        let Some(language) = buffer.syntax_language() else {
            if let Some(target) = self.end_session(id, window, cx) {
                self.show_message(&target, EMPTY_IMENU_MESSAGE, cx);
            }
            return;
        };
        let buf_snapshot = buffer.snapshot();
        self.session
            .as_mut()
            .unwrap()
            .candidates
            .as_mut()
            .unwrap()
            .loading = true;
        self.publish_candidates(cx);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let bg_cancelled = worker_cancelled.clone();
        let origin_target = origin.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    extract_from_rope(language, buf_snapshot.text(), &bg_cancelled)
                        .map(|syms| flatten(&syms))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.session.as_ref().is_some_and(|session| {
                        session.id() == id && session.origin() == &origin_target
                    }) {
                        return;
                    }
                    if let Some(mount) = &mut shell.minibuffer {
                        mount.imenu_work = None;
                    }
                    match result {
                        Ok(symbols) if symbols.is_empty() => {
                            if let Some(target) = shell.end_session(id, window, cx) {
                                shell.show_message(&target, EMPTY_IMENU_MESSAGE, cx);
                            }
                        }
                        Ok(symbols) => {
                            let default = origin_target
                                .read(cx)
                                .buffer()
                                .symbol_at_point()
                                .and_then(|sym| {
                                    symbols
                                        .iter()
                                        .find(|item| item.qualified_name == sym)
                                        .or_else(|| symbols.iter().find(|item| item.name == sym))
                                        .map(|item| item.qualified_name.clone())
                                });
                            if let Some(session) = &mut shell.session {
                                session.set_imenu_default(default);
                            }
                            let action = if let Some(mount) = &mut shell.minibuffer {
                                mount.imenu_symbols = Some(Arc::new(symbols));
                                mount.candidate_snapshot = None;
                                mount.pending_candidate_action.take()
                            } else {
                                None
                            };
                            shell.refresh_candidates(window, cx);
                            if let Some(action) = action {
                                shell.candidate_action(action, window, cx);
                            }
                        }
                        Err(error) => {
                            if !worker_cancelled.load(Ordering::Relaxed)
                                && let Some(target) = shell.end_session(id, window, cx)
                            {
                                shell.show_message(&target, error, cx);
                            }
                        }
                    }
                })
                .ok();
        });
        let _ = snapshot;
        self.minibuffer.as_mut().unwrap().imenu_work = Some(CompletionWork {
            cancelled,
            _task: task,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn refresh_project_imenu_candidates(
        &mut self,
        id: u64,
        project: Project,
        origin: &Entity<BufferView>,
        input: &str,
        snapshot: DisplayId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mount) = &mut self.minibuffer else {
            return;
        };
        let editor = mount.view.read(cx).editor().clone();
        if let Some(symbols) = mount.project_imenu_symbols.clone() {
            if symbols.len() <= 4096 {
                let items = filter_project_imenu_symbols(&symbols, input, &AtomicBool::new(false));
                self.session.as_mut().unwrap().candidates =
                    Some(CandidateList::ready(items, input, None, false));
                self.publish_candidates(cx);
            } else {
                self.session
                    .as_mut()
                    .unwrap()
                    .candidates
                    .as_mut()
                    .unwrap()
                    .loading = true;
                self.publish_candidates(cx);
                let cancelled = Arc::new(AtomicBool::new(false));
                let worker_cancelled = cancelled.clone();
                let owned_input = input.to_owned();
                let origin_target = origin.clone();
                let task = cx.spawn_in(window, async move |shell, cx| {
                    let items = cx
                        .background_executor()
                        .spawn(async move {
                            filter_project_imenu_symbols(&symbols, &owned_input, &worker_cancelled)
                        })
                        .await;
                    shell
                        .update_in(cx, |shell, window, cx| {
                            if !shell.session.as_ref().is_some_and(|session| {
                                session.id() == id && session.origin() == &origin_target
                            }) || !shell
                                .minibuffer
                                .as_ref()
                                .is_some_and(|mount| mount.candidate_snapshot == Some(snapshot))
                                || editor.read(cx).display_snapshot().id().committed
                                    != snapshot.committed
                                || editor.read(cx).display_snapshot().marked_range().is_some()
                            {
                                return;
                            }
                            let input = shell.session_input(cx).unwrap_or_default();
                            shell.session.as_mut().unwrap().candidates =
                                Some(CandidateList::ready(items, &input, None, false));
                            let mount = shell.minibuffer.as_mut().unwrap();
                            let action = mount.pending_candidate_action.take();
                            mount.completion = None;
                            shell.publish_candidates(cx);
                            if let Some(action) = action {
                                shell.candidate_action(action, window, cx);
                            }
                        })
                        .ok();
                });
                self.minibuffer.as_mut().unwrap().completion = Some(CompletionWork {
                    cancelled,
                    _task: task,
                });
            }
            return;
        }
        if mount.project_imenu_work.is_some() {
            return;
        }
        self.session
            .as_mut()
            .unwrap()
            .candidates
            .as_mut()
            .unwrap()
            .loading = true;
        self.publish_candidates(cx);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let open_files = self.collect_open_overlays();
        let cached_files = cx
            .global::<ProjectOutlineCache>()
            .snapshot_for(&project.root);
        let origin_target = origin.clone();
        #[cfg(test)]
        let gate = self.project_imenu_gate.take();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let bg_cancelled = worker_cancelled.clone();
            let scan_project_target = project.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    scan_project(
                        &scan_project_target,
                        &open_files,
                        &cached_files,
                        &bg_cancelled,
                    )
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.session.as_ref().is_some_and(|session| {
                        session.id() == id && session.origin() == &origin_target
                    }) {
                        return;
                    }
                    if let Some(mount) = &mut shell.minibuffer {
                        mount.project_imenu_work = None;
                    }
                    match result {
                        Ok(scan) => {
                            cx.global_mut::<ProjectOutlineCache>()
                                .install(project.root, scan.cache);
                            if scan.symbols.is_empty() {
                                if let Some(target) = shell.end_session(id, window, cx) {
                                    shell.show_message(&target, "No symbols found in project", cx);
                                }
                                return;
                            }
                            let action = if let Some(mount) = &mut shell.minibuffer {
                                mount.project_imenu_symbols = Some(Arc::new(scan.symbols));
                                mount.candidate_snapshot = None;
                                mount.pending_candidate_action.take()
                            } else {
                                None
                            };
                            shell.refresh_candidates(window, cx);
                            if let Some(action) = action {
                                shell.candidate_action(action, window, cx);
                            }
                        }
                        Err(error) => {
                            if !worker_cancelled.load(Ordering::Relaxed)
                                && let Some(target) = shell.end_session(id, window, cx)
                            {
                                shell.show_message(&target, error, cx);
                            }
                        }
                    }
                })
                .ok();
        });
        self.minibuffer.as_mut().unwrap().project_imenu_work = Some(CompletionWork {
            cancelled,
            _task: task,
        });
    }

    pub(super) fn confirm_imenu(
        &mut self,
        id: u64,
        default: Option<String>,
        accepted_id: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(symbols) = self
            .minibuffer
            .as_ref()
            .and_then(|mount| mount.imenu_symbols.clone())
        else {
            self.message_minibuffer("Loading index...", cx);
            return;
        };
        let input = self.session_input(cx).unwrap_or_default();
        let selected = if let Some(idx) = accepted_id {
            symbols.get(idx).cloned()
        } else {
            let query = if input.is_empty() {
                default.as_deref().unwrap_or("")
            } else {
                input.as_str()
            };
            if query.is_empty() {
                None
            } else {
                symbols
                    .iter()
                    .find(|item| item.qualified_name == query)
                    .or_else(|| symbols.iter().find(|item| item.name == query))
                    .cloned()
            }
        };
        let Some(sym) = selected else {
            self.message_minibuffer("No match", cx);
            return;
        };
        let Some(target) = self.end_session(id, window, cx) else {
            return;
        };
        target.update(cx, |view, cx| {
            let before = view.buffer().point();
            view.show_line_column(sym.line, sym.column, window, cx);
            let after = view.buffer().point();
            if after != before {
                view.save_mark(before, cx);
            }
        });
    }

    pub(super) fn confirm_project_imenu(
        &mut self,
        id: u64,
        accepted_id: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(symbols) = self
            .minibuffer
            .as_ref()
            .and_then(|mount| mount.project_imenu_symbols.clone())
        else {
            self.message_minibuffer("Scanning project symbols...", cx);
            return;
        };
        let input = self.session_input(cx).unwrap_or_default();
        let selected = if let Some(idx) = accepted_id {
            symbols.get(idx).cloned()
        } else if input.is_empty() {
            None
        } else {
            symbols
                .iter()
                .find(|item| item.qualified_name == input)
                .cloned()
        };
        let Some(sym) = selected else {
            self.message_minibuffer("No match", cx);
            return;
        };
        if self.end_session(id, window, cx).is_none() {
            return;
        }
        self.select_ordinary_for_visit(window, cx);
        let target = self.active_view().clone();
        if target.read(cx).buffer().path().as_deref() == Some(sym.path.as_path()) {
            target.update(cx, |view, cx| {
                let before = view.buffer().point();
                view.show_line_column(sym.line, sym.column, window, cx);
                let after = view.buffer().point();
                if after != before {
                    view.save_mark(before, cx);
                }
            });
        } else {
            self.visit_file_with(
                sym.path,
                &target,
                VisitOptions {
                    location: Some((sym.line, sym.column)),
                    ..Default::default()
                },
                window,
                cx,
            );
        }
    }
}

fn filter_imenu_symbols(
    symbols: &[FlatSymbol],
    query: &str,
    cancelled: &AtomicBool,
) -> Vec<MatchedCandidate> {
    let candidates: Vec<Candidate> = symbols
        .iter()
        .enumerate()
        .map(|(idx, sym)| {
            Candidate::named(
                sym.qualified_name.clone(),
                format!("{}  L{}", sym.kind.label(), sym.line),
            )
            .with_id(idx)
        })
        .collect();
    if query.is_empty() {
        completion::filter_unranked(candidates, query, cancelled)
    } else {
        completion::filter(candidates, query, cancelled)
    }
}

fn filter_project_imenu_symbols(
    symbols: &[ProjectSymbol],
    query: &str,
    cancelled: &AtomicBool,
) -> Vec<MatchedCandidate> {
    let candidates: Vec<Candidate> = symbols
        .iter()
        .enumerate()
        .map(|(idx, sym)| {
            Candidate::named(
                sym.qualified_name.clone(),
                format!("{}  {}:{}", sym.kind.label(), sym.relative_path, sym.line),
            )
            .with_id(idx)
        })
        .collect();
    if query.is_empty() {
        completion::filter_unranked(candidates, query, cancelled)
    } else {
        completion::filter(candidates, query, cancelled)
    }
}
