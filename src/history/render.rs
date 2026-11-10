use std::ops::Range;

use super::*;
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Commit(String),
    File {
        default: Option<Visit>,
        versions: Vec<FileVersion>,
    },
}

#[derive(Clone, Debug)]
enum Target {
    Commit(String),
    Unified {
        file: usize,
        old: Option<usize>,
        new: Option<usize>,
    },
    Combined {
        file: usize,
        hunk: Option<usize>,
        line: Option<usize>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Location {
    pub range: Range<ByteOffset>,
    prefix: Option<usize>,
    target: Target,
}

impl Location {
    pub fn commit_oid(&self) -> Option<&str> {
        match &self.target {
            Target::Commit(oid) => Some(oid),
            _ => None,
        }
    }

    pub fn action_at(
        &self,
        data: &Data,
        query: &Query,
        point: ByteOffset,
    ) -> Result<Action, String> {
        let column = self.prefix.map_or(0, |prefix| {
            point.0.saturating_sub(self.range.start.0 + prefix)
        });
        let repository = query.repository();
        let environment = query.environment();
        match (&self.target, data) {
            (Target::Commit(oid), _) => Ok(Action::Commit(oid.clone())),
            (
                Target::Unified { file, old, new },
                Data::Revision(Revision {
                    patch: Patch::Unified(files),
                    ..
                }),
            ) => {
                let file = files.get(*file).ok_or("No file here")?;
                let default = file
                    .visit(repository, *old, *new, column, false, environment)
                    .ok();
                let mut versions = Vec::new();
                if file.summary.is_none() {
                    for (name, path, oid, mode, line) in [
                        (
                            "result",
                            &file.new_path,
                            &file.new_oid,
                            &file.new_mode,
                            *new,
                        ),
                        (
                            "parent 1",
                            &file.old_path,
                            &file.old_oid,
                            &file.old_mode,
                            *old,
                        ),
                    ] {
                        if let Ok(visit) = (Blob {
                            path: path.clone(),
                            oid: oid.clone(),
                            mode: mode.clone(),
                        })
                        .visit(
                            repository,
                            line.unwrap_or(1),
                            column,
                            environment,
                        ) {
                            versions.push(FileVersion {
                                name: name.into(),
                                visit,
                            });
                        }
                    }
                }
                Ok(Action::File { default, versions })
            }
            (
                Target::Combined { file, hunk, line },
                Data::Revision(Revision {
                    patch: Patch::Combined(files),
                    ..
                }),
            ) => {
                let file = files.get(*file).ok_or("No file here")?;
                let initial = vec![1; file.parents.len()];
                let (parents, result) = if let Some(hunk) = hunk {
                    let hunk = file.hunks().get(*hunk).ok_or("No hunk here")?;
                    if let Some(line) = line {
                        let line = hunk.lines.get(*line).ok_or("No line here")?;
                        (line.parents.as_slice(), line.result)
                    } else {
                        (hunk.parents.as_slice(), hunk.result)
                    }
                } else {
                    (initial.as_slice(), 1)
                };
                let default = file
                    .result
                    .visit(repository, result, column, environment)
                    .ok();
                let mut versions = Vec::new();
                if let Some(visit) = &default {
                    versions.push(FileVersion {
                        name: "result".into(),
                        visit: visit.clone(),
                    });
                }
                for (index, parent) in file.parents.iter().enumerate() {
                    if let Ok(visit) = parent.visit(repository, parents[index], column, environment)
                    {
                        versions.push(FileVersion {
                            name: format!("parent {}", index + 1),
                            visit,
                        });
                    }
                }
                Ok(Action::File { default, versions })
            }
            _ => Err("File location does not belong to this revision".into()),
        }
    }
}

pub(crate) struct RenderedHistory {
    pub text: String,
    pub sections: Sections,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
    pub locations: Vec<Location>,
}

struct Builder {
    text: String,
    nodes: Vec<Section>,
    styles: Vec<(Range<ByteOffset>, StyleRole)>,
    locations: Vec<Location>,
}

impl Builder {
    fn new(title: &str) -> Self {
        let mut this = Self {
            text: String::new(),
            nodes: Vec::new(),
            styles: Vec::new(),
            locations: Vec::new(),
        };
        let range = this.line(title, StyleRole::SectionHeading);
        this.nodes.push(Section {
            key: "root".into(),
            parent: None,
            range: range.clone(),
            body: range.end,
            collapsed: false,
        });
        this
    }

    fn line(&mut self, text: &str, style: StyleRole) -> Range<ByteOffset> {
        let start = ByteOffset(self.text.len());
        self.text.push_str(text);
        if !text.ends_with('\n') {
            self.text.push('\n');
        }
        let range = start..ByteOffset(self.text.len());
        self.styles.push((range.clone(), style));
        range
    }

    fn leaf(&mut self, text: &str, key: String, style: StyleRole, target: Target) {
        let range = self.line(text, style);
        self.nodes.push(Section {
            key,
            parent: Some(0),
            range: range.clone(),
            body: range.end,
            collapsed: false,
        });
        self.locations.push(Location {
            range,
            prefix: None,
            target,
        });
    }

    fn finish(mut self) -> Result<RenderedHistory, String> {
        if self.text.len() > 16 * 1024 * 1024 {
            return Err("History projection exceeds 16 MiB".into());
        }
        self.nodes[0].range.end = ByteOffset(self.text.len());
        let sections = Sections::new(&Rope::from(self.text.as_str()), self.nodes)?;
        Ok(RenderedHistory {
            text: self.text,
            sections,
            styles: self.styles,
            locations: self.locations,
        })
    }
}

pub(super) fn render(data: &Data, query: &Query) -> Result<RenderedHistory, String> {
    match data {
        Data::Log(log) => {
            let mut out = Builder::new(&format!("Commits in {}", log.revision.escape_debug()));
            if matches!(query, Query::Log { select: true, .. }) {
                out.line(
                    "Select: C-c C-c / . / e; cancel: q / C-c C-k; RET shows commit",
                    StyleRole::Default,
                );
            }
            for entry in &log.entries {
                if let Some(commit) = &entry.commit {
                    out.leaf(
                        &format!(
                            "{}{} {}{}  {}  {}",
                            entry.graph,
                            &commit.oid[..12],
                            commit.message.escape_debug(),
                            if commit.decorations.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", commit.decorations.escape_debug())
                            },
                            commit.author.escape_debug(),
                            commit.date.escape_debug()
                        ),
                        format!("commit/{}", commit.oid),
                        StyleRole::Default,
                        Target::Commit(commit.oid.clone()),
                    );
                    let range = out.styles.pop().unwrap().0;
                    let hash_start = range.start.0 + entry.graph.len();
                    out.styles
                        .push((range.start..ByteOffset(hash_start), StyleRole::Default));
                    out.styles.push((
                        ByteOffset(hash_start)..ByteOffset(hash_start + 12),
                        StyleRole::MagitHash,
                    ));
                    out.styles
                        .push((ByteOffset(hash_start + 12)..range.end, StyleRole::Default));
                } else {
                    out.line(&entry.graph, StyleRole::Default);
                }
            }
            if log.entries.is_empty() {
                out.line("No commits", StyleRole::Default);
            }
            if log.more {
                out.line("Type + to show more history", StyleRole::SectionHeading);
            }
            out.finish()
        }
        Data::Revision(revision) => {
            let info = &revision.info;
            let mut out = Builder::new(&format!("Commit {}", info.oid));
            for (index, parent) in info.parents.iter().enumerate() {
                out.leaf(
                    &format!("Parent {}: {}", index + 1, parent),
                    format!("parent/{parent}"),
                    StyleRole::MagitHash,
                    Target::Commit(parent.clone()),
                );
            }
            out.line(
                &format!("Author: {}", info.author.escape_debug()),
                StyleRole::Default,
            );
            out.line(
                &format!("Date: {}", info.date.escape_debug()),
                StyleRole::Default,
            );
            out.line("", StyleRole::Default);
            for line in info.message.lines() {
                out.line(&line.escape_debug().to_string(), StyleRole::Default);
            }
            out.line("", StyleRole::Default);
            match &revision.patch {
                Patch::Unified(files) => append_unified(&mut out, files)?,
                Patch::Combined(files) => append_combined(&mut out, files),
            }
            out.finish()
        }
    }
}

fn append_unified(out: &mut Builder, files: &[FileDiff]) -> Result<(), String> {
    let rendered = crate::diff::render::render_files("Changes", files)?;
    let offset = out.text.len();
    let base = out.nodes.len();
    out.text.push_str(&rendered.text);
    for mut node in rendered.sections.nodes().iter().cloned() {
        node.parent = Some(node.parent.map_or(0, |parent| base + parent));
        node.range = ByteOffset(node.range.start.0 + offset)..ByteOffset(node.range.end.0 + offset);
        node.body = ByteOffset(node.body.0 + offset);
        node.key = format!("patch/{}", node.key);
        out.nodes.push(node);
    }
    out.styles
        .extend(rendered.styles.into_iter().map(|(range, role)| {
            (
                ByteOffset(range.start.0 + offset)..ByteOffset(range.end.0 + offset),
                role,
            )
        }));
    for location in rendered.navigation.locations.iter() {
        out.locations.push(Location {
            range: ByteOffset(location.range.start.0 + offset)
                ..ByteOffset(location.range.end.0 + offset),
            prefix: location.prefix,
            target: Target::Unified {
                file: location.file,
                old: location.old,
                new: location.new,
            },
        });
    }
    Ok(())
}

fn append_combined(out: &mut Builder, files: &[combined::File]) {
    out.line(
        &format!("{} combined files changed", files.len()),
        StyleRole::SectionHeading,
    );
    for (file_index, file) in files.iter().enumerate() {
        let key = format!("file/{:?}", file.result.path);
        let range = out.line(
            &format!(
                "Combined {}",
                file.result.path.to_string_lossy().escape_debug()
            ),
            StyleRole::DiffFile,
        );
        let parent = out.nodes.len();
        out.nodes.push(Section {
            key: key.clone(),
            parent: Some(0),
            range: range.clone(),
            body: range.end,
            collapsed: false,
        });
        if let combined::Body::Summary(summary) = &file.body {
            out.line(summary, StyleRole::Default);
        } else {
            out.locations.push(Location {
                range,
                prefix: None,
                target: Target::Combined {
                    file: file_index,
                    hunk: None,
                    line: None,
                },
            });
            if matches!(file.body, combined::Body::Omitted) {
                let range = out.line(
                    "Patch omitted by dense-combined diff; file versions remain available",
                    StyleRole::Default,
                );
                out.locations.push(Location {
                    range,
                    prefix: None,
                    target: Target::Combined {
                        file: file_index,
                        hunk: None,
                        line: None,
                    },
                });
            } else if file.hunks().is_empty() {
                out.line("Metadata only", StyleRole::Default);
            }
            for (hunk_index, hunk) in file.hunks().iter().enumerate() {
                let range = out.line(&hunk.header, StyleRole::DiffHunk);
                let index = out.nodes.len();
                out.nodes.push(Section {
                    key: format!("{key}/{}", hunk.header),
                    parent: Some(parent),
                    range: range.clone(),
                    body: range.end,
                    collapsed: false,
                });
                out.locations.push(Location {
                    range,
                    prefix: None,
                    target: Target::Combined {
                        file: file_index,
                        hunk: Some(hunk_index),
                        line: None,
                    },
                });
                for (line_index, line) in hunk.lines.iter().enumerate() {
                    let prefix = &line.text.as_bytes()[..file.parents.len().min(line.text.len())];
                    let role = if prefix.contains(&b'-') {
                        StyleRole::DiffRemoved
                    } else if prefix.contains(&b'+') {
                        StyleRole::DiffAdded
                    } else {
                        StyleRole::DiffContext
                    };
                    let range = out.line(&line.text, role);
                    if !line.text.starts_with('\\') {
                        out.locations.push(Location {
                            range,
                            prefix: (!line.text.starts_with('-')).then_some(file.parents.len()),
                            target: Target::Combined {
                                file: file_index,
                                hunk: Some(hunk_index),
                                line: Some(line_index),
                            },
                        });
                    }
                }
                out.nodes[index].range.end = ByteOffset(out.text.len());
            }
        }
        out.nodes[parent].range.end = ByteOffset(out.text.len());
    }
}
