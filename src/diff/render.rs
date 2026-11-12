use super::{Diff, FileDiff, Visit};
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct Location {
    pub range: Range<ByteOffset>,
    pub file: usize,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub prefix: Option<usize>,
}
#[derive(Clone, Debug)]
pub(crate) struct RenderedDiff {
    pub text: String,
    pub sections: Sections,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
    pub navigation: DiffNavigation,
}

#[derive(Clone, Debug)]
pub(crate) struct DiffNavigation {
    pub locations: Arc<[Location]>,
    pub peers: Vec<(ByteOffset, ByteOffset)>,
}
impl DiffNavigation {
    pub fn visit(&self, diff: &Diff, point: ByteOffset) -> Result<Visit, String> {
        let index = self
            .locations
            .partition_point(|location| location.range.end <= point);
        let location = self
            .locations
            .get(index)
            .filter(|location| location.range.contains(&point))
            .ok_or("No supported file location here")?;
        let column = location.prefix.map_or(0, |prefix| {
            point.0.saturating_sub(location.range.start.0 + prefix)
        });
        diff.visit(location.file, location.old, location.new, column)
    }
}
fn push(text: &mut String, value: &str) -> Range<ByteOffset> {
    let start = ByteOffset(text.len());
    text.push_str(value);
    if !value.ends_with('\n') {
        text.push('\n');
    }
    start..ByteOffset(text.len())
}
fn label(path: &std::path::Path) -> String {
    path.to_string_lossy().escape_debug().to_string()
}

pub(super) fn render(diff: &Diff) -> Result<RenderedDiff, String> {
    render_files(
        &format!(
            "{}: {}",
            diff.comparison.kind.name(),
            label(&diff.comparison.repository.root)
        ),
        &diff.files,
    )
}

pub(crate) fn render_files(title: &str, files: &[FileDiff]) -> Result<RenderedDiff, String> {
    let mut text = String::new();
    let title = push(&mut text, title);
    let mut nodes = vec![Section {
        key: "root".into(),
        parent: None,
        range: title.clone(),
        body: title.end,
        collapsed: false,
        hunk: false,
    }];
    let mut styles = vec![(title, StyleRole::SectionHeading)];
    let mut locations = Vec::new();
    let mut peers = Vec::new();
    if !files.is_empty() {
        let heading = push(&mut text, &format!("{} files changed", files.len()));
        styles.push((heading.clone(), StyleRole::SectionHeading));
        let stat = nodes.len();
        nodes.push(Section {
            key: "stat".into(),
            parent: Some(0),
            range: heading.clone(),
            body: heading.end,
            collapsed: false,
            hunk: false,
        });
        for (file, item) in files.iter().enumerate() {
            let added = format!("+{}", item.added.map_or("binary".into(), |n| n.to_string()));
            let deleted = format!(
                "-{}",
                item.deleted.map_or("binary".into(), |n| n.to_string())
            );
            let range = push(
                &mut text,
                &format!(" {} | {added} {deleted}", label(&item.new_path)),
            );
            let deleted_end = range.end.0 - 1;
            let deleted_start = deleted_end - deleted.len();
            let added_end = deleted_start - 1;
            styles.push((
                ByteOffset(added_end - added.len())..ByteOffset(added_end),
                StyleRole::DiffStatAdded,
            ));
            styles.push((
                ByteOffset(deleted_start)..ByteOffset(deleted_end),
                StyleRole::DiffStatRemoved,
            ));
            nodes.push(Section {
                key: format!("stat/{:?}", item.new_path),
                parent: Some(stat),
                range: range.clone(),
                body: range.end,
                collapsed: false,
                hunk: false,
            });
            locations.push(Location {
                range,
                file,
                old: (item.status == 'D').then_some(1),
                new: (item.status != 'D').then_some(1),
                prefix: None,
            });
        }
        nodes[stat].range.end = ByteOffset(text.len());
    } else {
        push(&mut text, "No changes");
    }
    for (file, item) in files.iter().enumerate() {
        let name = if item.old_path != item.new_path {
            format!("{} → {}", label(&item.old_path), label(&item.new_path))
        } else {
            label(&item.new_path)
        };
        let range = push(&mut text, &format!("{} {}", item.status, name));
        styles.push((range.clone(), StyleRole::DiffFile));
        let parent = nodes.len();
        let key = format!("file/{:?}", item.new_path);
        nodes.push(Section {
            key: key.clone(),
            parent: Some(0),
            range: range.clone(),
            body: range.end,
            collapsed: item.status == 'D',
            hunk: false,
        });
        let stat = locations
            .iter()
            .find(|location| location.file == file)
            .unwrap()
            .range
            .start;
        peers.push((stat, range.start));
        locations.push(Location {
            range,
            file,
            old: (item.status == 'D').then_some(1),
            new: (item.status != 'D').then_some(1),
            prefix: None,
        });
        if let Some(summary) = &item.summary {
            push(&mut text, summary);
        }
        if item.hunks.is_empty() && item.summary.is_none() {
            push(
                &mut text,
                &format!("Metadata only: mode {} → {}", item.old_mode, item.new_mode),
            );
        }
        for hunk in &item.hunks {
            let range = push(&mut text, &hunk.header);
            styles.push((range.clone(), StyleRole::DiffHunk));
            let index = nodes.len();
            nodes.push(Section {
                key: format!("{key}/{}", hunk.header),
                parent: Some(parent),
                range: range.clone(),
                body: range.end,
                collapsed: false,
                hunk: true,
            });
            locations.push(Location {
                range,
                file,
                old: (hunk.old_start > 0).then_some(hunk.old_start),
                new: (hunk.new_start > 0).then_some(hunk.new_start),
                prefix: None,
            });
            for line in &hunk.lines {
                let range = push(&mut text, &line.text);
                let role = match line.text.as_bytes().first() {
                    Some(b'+') => StyleRole::DiffAdded,
                    Some(b'-') => StyleRole::DiffRemoved,
                    _ => StyleRole::DiffContext,
                };
                styles.push((range.clone(), role));
                if line.old_line.is_some() || line.new_line.is_some() {
                    locations.push(Location {
                        range,
                        file,
                        old: line.old_line,
                        new: line.new_line,
                        prefix: Some(1),
                    });
                }
            }
            nodes[index].range.end = ByteOffset(text.len());
        }
        nodes[parent].range.end = ByteOffset(text.len());
    }
    nodes[0].range.end = ByteOffset(text.len());
    let sections = Sections::new(&Rope::from(text.as_str()), nodes)?;
    Ok(RenderedDiff {
        text,
        sections,
        styles,
        navigation: DiffNavigation {
            locations: locations.into(),
            peers,
        },
    })
}
