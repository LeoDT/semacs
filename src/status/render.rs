use std::fmt::Write;
use std::ops::Range;
use std::path::PathBuf;

use super::{Group, Head, MAX_BYTES, Record, Status};
use crate::diff::{DiffKind, Hunk, ReadEnvironment, Visit};
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLocation {
    pub range: Range<ByteOffset>,
    pub group: Group,
    pub entry: usize,
    pub hunk: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusAction {
    Diff(DiffKind),
    Visit(Visit),
    WorktreeFile {
        path: PathBuf,
        location: Option<(usize, usize)>,
    },
    Directory(PathBuf),
    Submodule(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusVisitKind {
    Group(Group),
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
    pub(crate) locations: Vec<StatusLocation>,
    pub(crate) visits: Vec<StatusVisitSpan>,
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
        match span.kind {
            StatusVisitKind::Group(Group::Unstaged) if thing => {
                Ok(StatusAction::Diff(DiffKind::Unstaged))
            }
            StatusVisitKind::Group(Group::Staged) if thing => {
                Ok(StatusAction::Diff(DiffKind::Staged))
            }
            StatusVisitKind::Group(_) => Err(if thing {
                "There is no thing at point that could be visited".into()
            } else {
                "No supported file location here".into()
            }),
            StatusVisitKind::File { group, entry } => self.visit_entry(group, entry, None, None),
            StatusVisitKind::HunkHeading { group, entry, hunk } => {
                self.visit_entry(group, entry, Some(hunk), None)
            }
            StatusVisitKind::HunkLine {
                group,
                entry,
                old,
                new,
                line_start,
            } => {
                let column = point.0.saturating_sub(line_start.0 + 1);
                self.visit_entry(group, entry, None, Some((old, new, column)))
            }
        }
    }

    fn visit_entry(
        &self,
        group: Group,
        entry_index: usize,
        hunk_index: Option<usize>,
        line: Option<(Option<usize>, Option<usize>, usize)>,
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
                let worktree = group == Group::Unstaged;
                if let Some(file) = self.file_diff(group, entry_index) {
                    if let Some((old_line, new_line, column)) = line {
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
                        return Err("This change has no supported text location".into());
                    }
                    let old = file.status == 'D';
                    if let Some(hunk) = file.hunks.first() {
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
                    if !old && worktree {
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
                if worktree && entry.xy[1] != b'D' {
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

pub(super) fn render(status: &Status) -> Result<RenderedStatus, String> {
    let mut text = String::new();
    let mut styles = Vec::new();
    append(&mut text, "Status: ")?;
    escaped(
        &mut text,
        status.repository.root.as_os_str().as_encoded_bytes(),
    )?;
    append(&mut text, "\nBranch: ")?;
    let branch_start = ByteOffset(text.len());
    match &status.head {
        Head::Branch { name, oid } => {
            escaped(&mut text, name)?;
            styles.push((branch_start..ByteOffset(text.len()), StyleRole::MagitBranch));
            append(&mut text, "\nHEAD: ")?;
            let start = ByteOffset(text.len());
            append(&mut text, &format!("{oid}\n"))?;
            styles.push((start..ByteOffset(text.len() - 1), StyleRole::MagitHash));
        }
        Head::Detached { oid } => {
            append(&mut text, "(detached)\nHEAD: ")?;
            let start = ByteOffset(text.len());
            append(&mut text, &format!("{oid}\n"))?;
            styles.push((start..ByteOffset(text.len() - 1), StyleRole::MagitHash));
        }
        Head::Unborn { name } => {
            escaped(&mut text, name)?;
            styles.push((branch_start..ByteOffset(text.len()), StyleRole::MagitBranch));
            append(&mut text, "\nHEAD: (unborn)\n")?;
        }
    }
    let title = ByteOffset(0)..ByteOffset(text.len());
    let mut nodes = vec![Section {
        key: "root".into(),
        parent: None,
        range: title.clone(),
        body: title.end,
        collapsed: false,
        hunk: false,
    }];
    let mut locations = Vec::new();
    let mut visits = Vec::new();
    for group in Group::ALL {
        let start = ByteOffset(text.len());
        append(
            &mut text,
            &format!("{} ({})\n", group.name(), status.group(group).count()),
        )?;
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
        for (index, entry) in status.group(group) {
            let start = ByteOffset(text.len());
            append(
                &mut text,
                &format!(" {}{} ", char::from(entry.xy[0]), char::from(entry.xy[1])),
            )?;
            if let Record::Tracked {
                rename: Some(rename),
                ..
            } = &entry.record
            {
                escaped(&mut text, rename.source.as_os_str().as_encoded_bytes())?;
                append(&mut text, " → ")?;
            }
            escaped(&mut text, entry.path.as_os_str().as_encoded_bytes())?;
            if matches!(entry.record, Record::Untracked { directory: true }) {
                append(&mut text, "/")?;
            }
            if entry.submodule.is_submodule {
                let flags = entry.submodule;
                append(
                    &mut text,
                    &format!(
                        " [submodule {}{}{}]",
                        if flags.commit_changed { 'C' } else { '.' },
                        if flags.tracked_dirty { 'M' } else { '.' },
                        if flags.untracked_dirty { 'U' } else { '.' },
                    ),
                )?;
            }
            append(&mut text, "\n")?;
            let range = start..ByteOffset(text.len());
            let file_node = nodes.len();
            let key = format!("{}/file/{}", group.key(), path_key(&entry.path));
            let diff = status.file_diff(group, index);
            if diff.is_some() {
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
                entry: index,
                hunk: None,
            });
            visits.push(StatusVisitSpan {
                range,
                kind: StatusVisitKind::File {
                    group,
                    entry: index,
                },
            });
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
                        entry: index,
                        hunk: Some(hunk_index),
                    });
                }
            }
            nodes[file_node].range.end = ByteOffset(text.len());
        }
        nodes[parent].range.end = ByteOffset(text.len());
    }
    nodes[0].range.end = ByteOffset(text.len());
    let sections = Sections::new(&Rope::from(text.as_str()), nodes)?;
    Ok(RenderedStatus {
        text,
        sections,
        styles,
        locations,
        visits,
    })
}
