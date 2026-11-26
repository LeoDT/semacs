use std::fmt::Write;
use std::ops::Range;
use std::path::PathBuf;

use super::{CommitSection, Group, Head, MAX_BYTES, Record, Status};
use crate::diff::{DiffKind, Hunk, ReadEnvironment, Visit};
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusTarget {
    Group,
    File { entry: usize },
    Hunk { entry: usize, hunk: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLocation {
    pub range: Range<ByteOffset>,
    pub group: Group,
    pub target: StatusTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusAction {
    Diff(DiffKind),
    DiffRange(String),
    Commit(String),
    Rev(String),
    Visit(Visit),
    WorktreeFile {
        path: PathBuf,
        location: Option<(usize, usize)>,
    },
    Directory(PathBuf),
    Submodule(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusPreviewTarget {
    Commit(String),
    Rev(String),
    Range(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusVisitKind {
    Group(Group),
    Commit(String),
    Rev(String),
    DiffRange(String),
    File {
        group: Group,
        entry: usize,
    },
    HunkHeading {
        group: Group,
        entry: usize,
        hunk: usize,
    },
    HunkLine {
        group: Group,
        entry: usize,
        old: Option<usize>,
        new: Option<usize>,
        line_start: ByteOffset,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusVisitSpan {
    pub range: Range<ByteOffset>,
    pub kind: StatusVisitKind,
}

#[derive(Clone, Debug)]
pub struct RenderedStatus {
    pub text: String,
    pub sections: Sections,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
    pub initial_point: ByteOffset,
    pub(crate) locations: Vec<StatusLocation>,
    pub(crate) visits: Vec<StatusVisitSpan>,
    pub(crate) copy_spans: Vec<(Range<ByteOffset>, String)>,
}

impl RenderedStatus {
    pub fn location(&self, point: ByteOffset) -> Option<&StatusLocation> {
        if self.sections.collapse_point(point) != point {
            return None;
        }
        let index = self
            .locations
            .partition_point(|location| location.range.end <= point);
        self.locations
            .get(index)
            .filter(|location| location.range.contains(&point))
    }
}

fn hunk_heading_line(hunk: &Hunk, old: bool) -> usize {
    let base = if old { hunk.old_start } else { hunk.new_start };
    if let Some(pos) = hunk
        .lines
        .iter()
        .position(|line| matches!(line.text.as_bytes().first(), Some(b'-' | b'+')))
    {
        let skip = if old { b'+' } else { b'-' };
        let offset = hunk.lines[..pos]
            .iter()
            .filter(|line| line.text.as_bytes().first() != Some(&skip))
            .count();
        return (base + offset).max(1);
    }
    base.max(1)
}

impl Status {
    pub(crate) fn visit(
        &self,
        visits: &[StatusVisitSpan],
        point: ByteOffset,
        thing: bool,
    ) -> Result<StatusAction, String> {
        let index = visits.partition_point(|span| span.range.end <= point);
        let Some(span) = visits.get(index).filter(|span| span.range.contains(&point)) else {
            return Err(if thing {
                "There is no thing at point that could be visited".into()
            } else {
                "No supported file location here".into()
            });
        };
        match &span.kind {
            StatusVisitKind::Group(Group::Unstaged) if thing => {
                Ok(StatusAction::Diff(DiffKind::Unstaged))
            }
            StatusVisitKind::Group(Group::Staged) if thing => {
                Ok(StatusAction::Diff(DiffKind::Staged))
            }
            StatusVisitKind::Commit(oid) if thing => Ok(StatusAction::Commit(oid.clone())),
            StatusVisitKind::Rev(rev) if thing => Ok(StatusAction::Rev(rev.clone())),
            StatusVisitKind::DiffRange(range) if thing => {
                Ok(StatusAction::DiffRange(range.clone()))
            }
            StatusVisitKind::Group(_)
            | StatusVisitKind::Commit(_)
            | StatusVisitKind::Rev(_)
            | StatusVisitKind::DiffRange(_) => Err(if thing {
                "There is no thing at point that could be visited".into()
            } else {
                "No supported file location here".into()
            }),
            StatusVisitKind::File { group, entry } => {
                self.visit_entry(*group, *entry, None, None, false)
            }
            StatusVisitKind::HunkHeading { group, entry, hunk } => {
                self.visit_entry(*group, *entry, Some(*hunk), None, false)
            }
            StatusVisitKind::HunkLine {
                group,
                entry,
                old,
                new,
                line_start,
            } => {
                let column = point.0.saturating_sub(line_start.0 + 1);
                self.visit_entry(*group, *entry, None, Some((*old, *new, column)), false)
            }
        }
    }

    pub(crate) fn visit_worktree(
        &self,
        visits: &[StatusVisitSpan],
        point: ByteOffset,
    ) -> Result<StatusAction, String> {
        let index = visits.partition_point(|span| span.range.end <= point);
        let Some(span) = visits.get(index).filter(|span| span.range.contains(&point)) else {
            return Err("No file at point".into());
        };
        match &span.kind {
            StatusVisitKind::File { group, entry } => {
                self.visit_entry(*group, *entry, None, None, true)
            }
            StatusVisitKind::HunkHeading { group, entry, hunk } => {
                self.visit_entry(*group, *entry, Some(*hunk), None, true)
            }
            StatusVisitKind::HunkLine {
                group,
                entry,
                old,
                new,
                line_start,
            } => {
                let column = point.0.saturating_sub(line_start.0 + 1);
                self.visit_entry(*group, *entry, None, Some((*old, *new, column)), true)
            }
            _ => Err("No file at point".into()),
        }
    }

    pub(crate) fn preview_target_at(
        visits: &[StatusVisitSpan],
        point: ByteOffset,
    ) -> Option<StatusPreviewTarget> {
        let index = visits.partition_point(|span| span.range.end <= point);
        let span = visits
            .get(index)
            .filter(|span| span.range.contains(&point))?;
        match &span.kind {
            StatusVisitKind::Commit(oid) => Some(StatusPreviewTarget::Commit(oid.clone())),
            StatusVisitKind::Rev(rev) => Some(StatusPreviewTarget::Rev(rev.clone())),
            StatusVisitKind::DiffRange(range) => Some(StatusPreviewTarget::Range(range.clone())),
            _ => None,
        }
    }

    fn visit_entry(
        &self,
        group: Group,
        entry_index: usize,
        hunk_index: Option<usize>,
        line: Option<(Option<usize>, Option<usize>, usize)>,
        force_worktree: bool,
    ) -> Result<StatusAction, String> {
        let entry = self
            .entries
            .get(entry_index)
            .ok_or("No supported file location here")?;
        if entry.submodule.is_submodule {
            return Ok(StatusAction::Submodule(
                self.repository.root.join(&entry.path),
            ));
        }
        match group {
            Group::Untracked => match entry.record {
                Record::Untracked { directory: true } => Ok(StatusAction::Directory(
                    self.repository.root.join(&entry.path),
                )),
                _ => Ok(StatusAction::WorktreeFile {
                    path: self.repository.root.join(&entry.path),
                    location: None,
                }),
            },
            Group::Unmerged => Ok(StatusAction::WorktreeFile {
                path: self.repository.root.join(&entry.path),
                location: None,
            }),
            Group::Unstaged | Group::Staged => {
                let worktree = force_worktree || group == Group::Unstaged;
                if let Some(file) = self.file_diff(group, entry_index) {
                    if let Some((old_line, new_line, column)) = line {
                        if force_worktree {
                            let target_line = new_line.or(old_line).unwrap_or(1).max(1);
                            return Ok(StatusAction::WorktreeFile {
                                path: self.repository.root.join(&file.new_path),
                                location: Some((target_line, column)),
                            });
                        }
                        return file
                            .visit(
                                &self.repository,
                                old_line,
                                new_line,
                                column,
                                worktree,
                                ReadEnvironment::Projection,
                            )
                            .map(StatusAction::Visit);
                    }
                    if let Some(hunk_index) = hunk_index {
                        let hunk = file
                            .hunks
                            .get(hunk_index)
                            .ok_or("No supported file location here")?;
                        if force_worktree {
                            let target_line = hunk_heading_line(hunk, false);
                            return Ok(StatusAction::WorktreeFile {
                                path: self.repository.root.join(&file.new_path),
                                location: Some((target_line, 0)),
                            });
                        }
                        let old = file.status == 'D';
                        let target_line = hunk_heading_line(hunk, old);
                        let (old_line, new_line) = if old {
                            (Some(target_line), None)
                        } else {
                            (None, Some(target_line))
                        };
                        return file
                            .visit(
                                &self.repository,
                                old_line,
                                new_line,
                                0,
                                worktree,
                                ReadEnvironment::Projection,
                            )
                            .map(StatusAction::Visit);
                    }
                    if file.summary.is_some() && file.hunks.is_empty() {
                        if force_worktree {
                            return Ok(StatusAction::WorktreeFile {
                                path: self.repository.root.join(&file.new_path),
                                location: None,
                            });
                        }
                        return Err("This change has no supported text location".into());
                    }
                    let old = file.status == 'D';
                    if let Some(hunk) = file.hunks.first() {
                        if force_worktree {
                            let target_line = hunk_heading_line(hunk, false);
                            return Ok(StatusAction::WorktreeFile {
                                path: self.repository.root.join(&file.new_path),
                                location: Some((target_line, 0)),
                            });
                        }
                        let target_line = hunk_heading_line(hunk, old);
                        let (old_line, new_line) = if old {
                            (Some(target_line), None)
                        } else {
                            (None, Some(target_line))
                        };
                        return file
                            .visit(
                                &self.repository,
                                old_line,
                                new_line,
                                0,
                                worktree,
                                ReadEnvironment::Projection,
                            )
                            .map(StatusAction::Visit);
                    }
                    if (!old && worktree) || force_worktree {
                        return Ok(StatusAction::WorktreeFile {
                            path: self.repository.root.join(&file.new_path),
                            location: None,
                        });
                    }
                    return file
                        .visit(
                            &self.repository,
                            old.then_some(1),
                            (!old).then_some(1),
                            0,
                            worktree,
                            ReadEnvironment::Projection,
                        )
                        .map(StatusAction::Visit);
                }
                if (worktree && entry.xy[1] != b'D') || force_worktree {
                    return Ok(StatusAction::WorktreeFile {
                        path: self.repository.root.join(&entry.path),
                        location: None,
                    });
                }
                if let Record::Tracked { state, rename } = &entry.record {
                    let deleted = if worktree {
                        entry.xy[1] == b'D'
                    } else {
                        entry.xy[0] == b'D'
                    };
                    let oid = if !worktree && deleted {
                        &state.head_oid
                    } else {
                        &state.index_oid
                    };
                    if oid.bytes().all(|byte| byte == b'0') {
                        return Err("No file version exists on this side".into());
                    }
                    let path = if deleted {
                        rename.as_ref().map_or(&entry.path, |rename| &rename.source)
                    } else {
                        &entry.path
                    };
                    return Ok(StatusAction::Visit(Visit::Blob {
                        root: self.repository.root.clone(),
                        git_dir: self.repository.git_dir.clone(),
                        oid: oid.clone(),
                        path: path.clone(),
                        environment: ReadEnvironment::Projection,
                        line: 1,
                        column: 0,
                    }));
                }
                Err("No supported file location here".into())
            }
        }
    }
}

fn append(text: &mut String, value: &str) -> Result<(), String> {
    if text.len().saturating_add(value.len()) > MAX_BYTES {
        return Err("Status exceeds the 16 MiB projection limit".into());
    }
    text.push_str(value);
    Ok(())
}

fn escaped(text: &mut String, bytes: &[u8]) -> Result<(), String> {
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            let mut buffer = [0; 4];
            for ch in ch.escape_debug() {
                append(text, ch.encode_utf8(&mut buffer))?;
            }
        }
        for byte in chunk.invalid() {
            append(text, &format!("\\x{byte:02x}"))?;
        }
    }
    Ok(())
}

fn path_key(path: &std::path::Path) -> String {
    let mut key = String::new();
    for byte in path.as_os_str().as_encoded_bytes() {
        write!(key, "{byte:02x}").unwrap();
    }
    key
}

fn status_word(code: u8) -> &'static str {
    match code {
        b'M' => "modified",
        b'A' => "new file",
        b'D' => "deleted",
        b'R' => "renamed",
        b'C' => "copied",
        b'T' => "typechange",
        _ => "modified",
    }
}

fn unmerged_suffix(xy: [u8; 2]) -> &'static str {
    match xy {
        [b'D', b'D'] => " (both deleted)",
        [b'D', b'U'] => " (deleted by us)",
        [b'U', b'D'] => " (deleted by them)",
        [b'A', b'A'] => " (both added)",
        [b'A', b'U'] => " (added by us)",
        [b'U', b'A'] => " (added by them)",
        _ => "",
    }
}

fn submodule_suffix(flags: super::Submodule) -> String {
    let mut parts = Vec::new();
    if flags.commit_changed {
        parts.push("new commits");
    }
    if flags.tracked_dirty {
        parts.push("modified content");
    }
    if flags.untracked_dirty {
        parts.push("untracked content");
    }
    if parts.is_empty() {
        " (submodule)".into()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

#[allow(clippy::too_many_arguments)]
fn render_commit_section(
    section: &CommitSection,
    text: &mut String,
    nodes: &mut Vec<Section>,
    styles: &mut Vec<(Range<ByteOffset>, StyleRole)>,
    visits: &mut Vec<StatusVisitSpan>,
    copy_spans: &mut Vec<(Range<ByteOffset>, String)>,
    has_previous_block: &mut bool,
    initial_point: &mut Option<ByteOffset>,
) -> Result<(), String> {
    if section.commits.is_empty() {
        return Ok(());
    }
    if *has_previous_block {
        append(text, "\n")?;
    }
    *has_previous_block = true;
    let start = ByteOffset(text.len());
    initial_point.get_or_insert(start);
    if section.show_count {
        append(
            text,
            &format!("{} ({})\n", section.title, section.commits.len()),
        )?;
    } else {
        append(text, &format!("{}\n", section.title))?;
    }
    let heading = start..ByteOffset(text.len());
    styles.push((
        start..ByteOffset(start.0 + section.title.len()),
        StyleRole::SectionHeading,
    ));
    if let Some(range) = &section.range {
        visits.push(StatusVisitSpan {
            range: heading.clone(),
            kind: StatusVisitKind::DiffRange(range.clone()),
        });
    }
    let parent = nodes.len();
    nodes.push(Section {
        key: section.key.clone(),
        parent: Some(0),
        range: heading.clone(),
        body: heading.end,
        collapsed: section.collapsed_by_default,
        hunk: false,
    });
    for commit in &section.commits {
        let c_start = ByteOffset(text.len());
        let short_start = ByteOffset(text.len());
        append(text, &commit.short)?;
        styles.push((short_start..ByteOffset(text.len()), StyleRole::MagitHash));
        append(text, " ")?;
        if !commit.decorations.is_empty() {
            let dec_start = ByteOffset(text.len());
            append(text, &format!("({})", commit.decorations))?;
            styles.push((dec_start..ByteOffset(text.len()), StyleRole::MagitBranch));
            append(text, " ")?;
        }
        append(text, &commit.summary)?;
        append(text, "\n")?;
        let c_range = c_start..ByteOffset(text.len());
        nodes.push(Section {
            key: format!("{}/commit/{}", section.key, commit.oid),
            parent: Some(parent),
            range: c_range.clone(),
            body: c_range.end,
            collapsed: false,
            hunk: false,
        });
        visits.push(StatusVisitSpan {
            range: c_range.clone(),
            kind: StatusVisitKind::Commit(commit.oid.clone()),
        });
        copy_spans.push((c_range, commit.short.clone()));
    }
    nodes[parent].range.end = ByteOffset(text.len());
    Ok(())
}

pub(super) fn render(status: &Status) -> Result<RenderedStatus, String> {
    let mut text = String::new();
    let mut styles = Vec::new();
    let mut nodes = vec![Section {
        key: "root".into(),
        parent: None,
        range: ByteOffset(0)..ByteOffset(0),
        body: ByteOffset(0),
        collapsed: false,
        hunk: false,
    }];
    let mut locations = Vec::new();
    let mut visits = Vec::new();
    let mut copy_spans = Vec::new();
    let mut has_previous_block;
    let mut initial_point: Option<ByteOffset> = None;

    match &status.head {
        Head::Unborn { .. } => {
            append(&mut text, "In the beginning there was darkness\n")?;
            nodes[0].body = ByteOffset(text.len());
            has_previous_block = true;
        }
        Head::Branch { name, oid } => {
            let head_start = ByteOffset(text.len());
            append(&mut text, "Head:     ")?;
            let branch_start = ByteOffset(text.len());
            escaped(&mut text, name)?;
            let branch_name = text[branch_start.0..].to_owned();
            styles.push((branch_start..ByteOffset(text.len()), StyleRole::MagitBranch));
            if let Some(summary) = &status.meta.head_summary {
                append(&mut text, " ")?;
                append(&mut text, summary)?;
            }
            append(&mut text, "\n")?;
            let head_heading = head_start..ByteOffset(text.len());
            let head_node = nodes.len();
            nodes.push(Section {
                key: "header/head".into(),
                parent: Some(0),
                range: head_heading.clone(),
                body: head_heading.end,
                collapsed: false,
                hunk: false,
            });
            visits.push(StatusVisitSpan {
                range: head_heading.clone(),
                kind: StatusVisitKind::Commit(oid.clone()),
            });
            copy_spans.push((head_heading, branch_name));

            if let Some(upstream) = &status.meta.upstream {
                let u_start = ByteOffset(text.len());
                append(&mut text, &format!("{:<10}", upstream.label))?;
                if !upstream.target.is_empty() {
                    let t_start = ByteOffset(text.len());
                    append(&mut text, &upstream.target)?;
                    styles.push((t_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                }
                if let Some(summary) = &upstream.summary {
                    append(&mut text, " ")?;
                    append(&mut text, summary)?;
                } else if let Some(warning) = &upstream.warning {
                    if !upstream.target.is_empty() {
                        append(&mut text, " ")?;
                    }
                    append(&mut text, warning)?;
                }
                append(&mut text, "\n")?;
                let u_range = u_start..ByteOffset(text.len());
                nodes.push(Section {
                    key: "header/upstream".into(),
                    parent: Some(head_node),
                    range: u_range.clone(),
                    body: u_range.end,
                    collapsed: false,
                    hunk: false,
                });
                if let Some(u_oid) = &upstream.oid {
                    visits.push(StatusVisitSpan {
                        range: u_range.clone(),
                        kind: StatusVisitKind::Commit(u_oid.clone()),
                    });
                }
                if !upstream.target.is_empty() {
                    copy_spans.push((u_range, upstream.target.clone()));
                }
            }

            if let Some(push) = &status.meta.push {
                let p_start = ByteOffset(text.len());
                append(&mut text, &format!("{:<10}", push.label))?;
                let t_start = ByteOffset(text.len());
                append(&mut text, &push.target)?;
                styles.push((t_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                if let Some(summary) = &push.summary {
                    append(&mut text, " ")?;
                    append(&mut text, summary)?;
                } else if let Some(warning) = &push.warning {
                    append(&mut text, " ")?;
                    append(&mut text, warning)?;
                }
                append(&mut text, "\n")?;
                let p_range = p_start..ByteOffset(text.len());
                nodes.push(Section {
                    key: "header/push".into(),
                    parent: Some(head_node),
                    range: p_range.clone(),
                    body: p_range.end,
                    collapsed: false,
                    hunk: false,
                });
                if let Some(p_oid) = &push.oid {
                    visits.push(StatusVisitSpan {
                        range: p_range.clone(),
                        kind: StatusVisitKind::Commit(p_oid.clone()),
                    });
                }
                copy_spans.push((p_range, push.target.clone()));
            }

            if status.meta.current_tag.is_some() || status.meta.next_tag.is_some() {
                let t_start = ByteOffset(text.len());
                let both = status.meta.current_tag.is_some() && status.meta.next_tag.is_some();
                append(&mut text, if both { "Tags:     " } else { "Tag:      " })?;
                if let Some(curr) = &status.meta.current_tag {
                    let n_start = ByteOffset(text.len());
                    append(&mut text, &curr.name)?;
                    styles.push((n_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                    if curr.count > 0 {
                        append(&mut text, &format!(" ({})", curr.count))?;
                    }
                }
                if both {
                    append(&mut text, ", ")?;
                }
                if let Some(next) = &status.meta.next_tag {
                    let n_start = ByteOffset(text.len());
                    append(&mut text, &next.name)?;
                    styles.push((n_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                    if next.count > 0 {
                        append(&mut text, &format!(" ({})", next.count))?;
                    }
                }
                append(&mut text, "\n")?;
                let t_range = t_start..ByteOffset(text.len());
                let tag_name = status
                    .meta
                    .current_tag
                    .as_ref()
                    .or(status.meta.next_tag.as_ref())
                    .unwrap()
                    .name
                    .clone();
                nodes.push(Section {
                    key: "header/tags".into(),
                    parent: Some(head_node),
                    range: t_range.clone(),
                    body: t_range.end,
                    collapsed: false,
                    hunk: false,
                });
                visits.push(StatusVisitSpan {
                    range: t_range.clone(),
                    kind: StatusVisitKind::Rev(tag_name.clone()),
                });
                copy_spans.push((t_range, tag_name));
            }

            nodes[head_node].range.end = ByteOffset(text.len());
            has_previous_block = true;
        }
        Head::Detached { oid } => {
            let head_start = ByteOffset(text.len());
            append(&mut text, "Head:     ")?;
            let short = status
                .meta
                .head_short
                .as_deref()
                .unwrap_or_else(|| &oid[..7.min(oid.len())]);
            let hash_start = ByteOffset(text.len());
            append(&mut text, short)?;
            styles.push((hash_start..ByteOffset(text.len()), StyleRole::MagitHash));
            if let Some(summary) = &status.meta.head_summary {
                append(&mut text, " ")?;
                append(&mut text, summary)?;
            }
            append(&mut text, "\n")?;
            let head_heading = head_start..ByteOffset(text.len());
            let head_node = nodes.len();
            nodes.push(Section {
                key: "header/head".into(),
                parent: Some(0),
                range: head_heading.clone(),
                body: head_heading.end,
                collapsed: false,
                hunk: false,
            });
            visits.push(StatusVisitSpan {
                range: head_heading.clone(),
                kind: StatusVisitKind::Commit(oid.clone()),
            });
            copy_spans.push((head_heading, short.to_owned()));

            if status.meta.current_tag.is_some() || status.meta.next_tag.is_some() {
                let t_start = ByteOffset(text.len());
                let both = status.meta.current_tag.is_some() && status.meta.next_tag.is_some();
                append(&mut text, if both { "Tags:     " } else { "Tag:      " })?;
                if let Some(curr) = &status.meta.current_tag {
                    let n_start = ByteOffset(text.len());
                    append(&mut text, &curr.name)?;
                    styles.push((n_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                    if curr.count > 0 {
                        append(&mut text, &format!(" ({})", curr.count))?;
                    }
                }
                if both {
                    append(&mut text, ", ")?;
                }
                if let Some(next) = &status.meta.next_tag {
                    let n_start = ByteOffset(text.len());
                    append(&mut text, &next.name)?;
                    styles.push((n_start..ByteOffset(text.len()), StyleRole::MagitBranch));
                    if next.count > 0 {
                        append(&mut text, &format!(" ({})", next.count))?;
                    }
                }
                append(&mut text, "\n")?;
                let t_range = t_start..ByteOffset(text.len());
                let tag_name = status
                    .meta
                    .current_tag
                    .as_ref()
                    .or(status.meta.next_tag.as_ref())
                    .unwrap()
                    .name
                    .clone();
                nodes.push(Section {
                    key: "header/tags".into(),
                    parent: Some(head_node),
                    range: t_range.clone(),
                    body: t_range.end,
                    collapsed: false,
                    hunk: false,
                });
                visits.push(StatusVisitSpan {
                    range: t_range.clone(),
                    kind: StatusVisitKind::Rev(tag_name.clone()),
                });
                copy_spans.push((t_range, tag_name));
            }

            nodes[head_node].range.end = ByteOffset(text.len());
            has_previous_block = true;
        }
    }

    if let Some(merge_section) = &status.meta.merge_section {
        render_commit_section(
            merge_section,
            &mut text,
            &mut nodes,
            &mut styles,
            &mut visits,
            &mut copy_spans,
            &mut has_previous_block,
            &mut initial_point,
        )?;
    }

    for group in Group::ALL {
        let count = status.group(group).count();
        if count == 0 {
            continue;
        }
        if has_previous_block {
            append(&mut text, "\n")?;
        }
        has_previous_block = true;
        let start = ByteOffset(text.len());
        initial_point.get_or_insert(start);
        append(&mut text, &format!("{} ({count})\n", group.name()))?;
        let heading = start..ByteOffset(text.len());
        let label_end = start.0 + group.name().len();
        styles.push((start..ByteOffset(label_end), StyleRole::SectionHeading));
        visits.push(StatusVisitSpan {
            range: heading.clone(),
            kind: StatusVisitKind::Group(group),
        });
        let parent = nodes.len();
        nodes.push(Section {
            key: group.key().into(),
            parent: Some(0),
            range: heading.clone(),
            body: heading.end,
            collapsed: false,
            hunk: false,
        });
        locations.push(StatusLocation {
            range: heading.clone(),
            group,
            target: StatusTarget::Group,
        });
        for (index, entry) in status.group(group) {
            let start = ByteOffset(text.len());
            match group {
                Group::Untracked => {
                    escaped(&mut text, entry.path.as_os_str().as_encoded_bytes())?;
                    if matches!(entry.record, Record::Untracked { directory: true }) {
                        append(&mut text, "/")?;
                    }
                }
                Group::Unmerged => {
                    append(&mut text, &format!("{:<11}", "unmerged"))?;
                    escaped(&mut text, entry.path.as_os_str().as_encoded_bytes())?;
                    append(&mut text, unmerged_suffix(entry.xy))?;
                }
                Group::Unstaged | Group::Staged => {
                    let code = if group == Group::Unstaged {
                        entry.xy[1]
                    } else {
                        entry.xy[0]
                    };
                    append(&mut text, &format!("{:<11}", status_word(code)))?;
                    if let Record::Tracked {
                        rename: Some(rename),
                        ..
                    } = &entry.record
                    {
                        escaped(&mut text, rename.source.as_os_str().as_encoded_bytes())?;
                        append(&mut text, " -> ")?;
                    }
                    escaped(&mut text, entry.path.as_os_str().as_encoded_bytes())?;
                    if entry.submodule.is_submodule {
                        append(&mut text, &submodule_suffix(entry.submodule))?;
                    }
                }
            }
            append(&mut text, "\n")?;
            let range = start..ByteOffset(text.len());
            let file_node = nodes.len();
            let key = format!("{}/file/{}", group.key(), path_key(&entry.path));
            let diff = status.file_diff(group, index);
            if matches!(group, Group::Unstaged | Group::Staged | Group::Unmerged) {
                styles.push((range.clone(), StyleRole::DiffFile));
            }
            nodes.push(Section {
                key: key.clone(),
                parent: Some(parent),
                range: range.clone(),
                body: range.end,
                collapsed: diff.is_some(),
                hunk: false,
            });
            locations.push(StatusLocation {
                range: range.clone(),
                group,
                target: StatusTarget::File { entry: index },
            });
            visits.push(StatusVisitSpan {
                range: range.clone(),
                kind: StatusVisitKind::File {
                    group,
                    entry: index,
                },
            });
            copy_spans.push((range, entry.path.to_string_lossy().into_owned()));
            if let Some(file) = diff {
                if let Some(summary) = &file.summary {
                    append(&mut text, summary)?;
                    append(&mut text, "\n")?;
                } else if file.hunks.is_empty() {
                    append(
                        &mut text,
                        &format!(
                            "Metadata only: mode {} → {}\n",
                            file.old_mode, file.new_mode
                        ),
                    )?;
                }
                if file.hunks.is_empty() {
                    let end = ByteOffset(text.len());
                    locations.last_mut().unwrap().range.end = end;
                    visits.last_mut().unwrap().range.end = end;
                }
                for (hunk_index, hunk) in file.hunks.iter().enumerate() {
                    let start = ByteOffset(text.len());
                    append(&mut text, &hunk.header)?;
                    append(&mut text, "\n")?;
                    let heading = start..ByteOffset(text.len());
                    styles.push((heading.clone(), StyleRole::DiffHunk));
                    visits.push(StatusVisitSpan {
                        range: heading.clone(),
                        kind: StatusVisitKind::HunkHeading {
                            group,
                            entry: index,
                            hunk: hunk_index,
                        },
                    });
                    let node = nodes.len();
                    nodes.push(Section {
                        key: format!("{key}/{}", hunk.header),
                        parent: Some(file_node),
                        range: heading.clone(),
                        body: heading.end,
                        collapsed: false,
                        hunk: true,
                    });
                    for line in &hunk.lines {
                        let start = ByteOffset(text.len());
                        append(&mut text, &line.text)?;
                        if !line.text.ends_with('\n') {
                            append(&mut text, "\n")?;
                        }
                        let line_range = start..ByteOffset(text.len());
                        let role = match line.text.as_bytes().first() {
                            Some(b'+') => StyleRole::DiffAdded,
                            Some(b'-') => StyleRole::DiffRemoved,
                            _ => StyleRole::DiffContext,
                        };
                        styles.push((line_range.clone(), role));
                        if line.old_line.is_some() || line.new_line.is_some() {
                            visits.push(StatusVisitSpan {
                                range: line_range,
                                kind: StatusVisitKind::HunkLine {
                                    group,
                                    entry: index,
                                    old: line.old_line,
                                    new: line.new_line,
                                    line_start: start,
                                },
                            });
                        }
                    }
                    let range = heading.start..ByteOffset(text.len());
                    nodes[node].range.end = range.end;
                    locations.push(StatusLocation {
                        range,
                        group,
                        target: StatusTarget::Hunk {
                            entry: index,
                            hunk: hunk_index,
                        },
                    });
                }
            }
            nodes[file_node].range.end = ByteOffset(text.len());
        }
        nodes[parent].range.end = ByteOffset(text.len());
    }

    if !status.meta.stashes.is_empty() {
        if has_previous_block {
            append(&mut text, "\n")?;
        }
        has_previous_block = true;
        let start = ByteOffset(text.len());
        initial_point.get_or_insert(start);
        append(
            &mut text,
            &format!("Stashes ({})\n", status.meta.stashes.len()),
        )?;
        let heading = start..ByteOffset(text.len());
        styles.push((
            start..ByteOffset(start.0 + "Stashes".len()),
            StyleRole::SectionHeading,
        ));
        let parent = nodes.len();
        nodes.push(Section {
            key: "stashes".into(),
            parent: Some(0),
            range: heading.clone(),
            body: heading.end,
            collapsed: true,
            hunk: false,
        });
        for stash in &status.meta.stashes {
            let s_start = ByteOffset(text.len());
            append(&mut text, &stash.ref_name)?;
            styles.push((s_start..ByteOffset(text.len()), StyleRole::MagitHash));
            append(&mut text, " ")?;
            append(&mut text, &stash.summary)?;
            append(&mut text, "\n")?;
            let s_range = s_start..ByteOffset(text.len());
            nodes.push(Section {
                key: format!("stashes/{}", stash.ref_name),
                parent: Some(parent),
                range: s_range.clone(),
                body: s_range.end,
                collapsed: false,
                hunk: false,
            });
            visits.push(StatusVisitSpan {
                range: s_range.clone(),
                kind: StatusVisitKind::Commit(stash.oid.clone()),
            });
            copy_spans.push((s_range, stash.ref_name.clone()));
        }
        nodes[parent].range.end = ByteOffset(text.len());
    }

    for section in &status.meta.log_sections {
        render_commit_section(
            section,
            &mut text,
            &mut nodes,
            &mut styles,
            &mut visits,
            &mut copy_spans,
            &mut has_previous_block,
            &mut initial_point,
        )?;
    }

    nodes[0].range.end = ByteOffset(text.len());
    let sections = Sections::new(&Rope::from(text.as_str()), nodes)?;
    Ok(RenderedStatus {
        text,
        sections,
        styles,
        initial_point: initial_point.unwrap_or(ByteOffset(0)),
        locations,
        visits,
        copy_spans,
    })
}
