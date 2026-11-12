use std::fmt::Write;
use std::ops::Range;

use super::{Group, Head, MAX_BYTES, Record, Status};
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

#[derive(Clone, Debug)]
pub struct RenderedStatus {
    pub text: String,
    pub sections: Sections,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
    pub(crate) locations: Vec<StatusLocation>,
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
    for group in Group::ALL {
        let start = ByteOffset(text.len());
        append(
            &mut text,
            &format!("{} ({})\n", group.name(), status.group(group).count()),
        )?;
        let heading = start..ByteOffset(text.len());
        let label_end = start.0 + group.name().len();
        styles.push((start..ByteOffset(label_end), StyleRole::SectionHeading));
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
                range,
                group,
                entry: index,
                hunk: None,
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
                    locations.last_mut().unwrap().range.end = ByteOffset(text.len());
                }
                for (hunk_index, hunk) in file.hunks.iter().enumerate() {
                    let start = ByteOffset(text.len());
                    append(&mut text, &hunk.header)?;
                    append(&mut text, "\n")?;
                    let heading = start..ByteOffset(text.len());
                    styles.push((heading.clone(), StyleRole::DiffHunk));
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
                        let role = match line.text.as_bytes().first() {
                            Some(b'+') => StyleRole::DiffAdded,
                            Some(b'-') => StyleRole::DiffRemoved,
                            _ => StyleRole::DiffContext,
                        };
                        styles.push((start..ByteOffset(text.len()), role));
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
    })
}
