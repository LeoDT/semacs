use super::{DiffLine, FileDiff, Hunk};
use crate::git::relative_path as path;

fn nul<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("Truncated Git metadata")?;
    let value = &bytes[..end];
    *bytes = &bytes[end + 1..];
    Ok(value)
}
fn utf8(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "Diff contains non-UTF-8 text".into())
}

#[cfg(test)]
pub(super) fn parse(bytes: &[u8]) -> Result<Vec<FileDiff>, String> {
    parse_text(bytes, false)
}

pub(super) fn parse_text(bytes: &[u8], summarize_non_utf8: bool) -> Result<Vec<FileDiff>, String> {
    parse_with_stats(bytes, summarize_non_utf8, true)
}

pub(crate) fn parse_revision(bytes: &[u8]) -> Result<Vec<FileDiff>, String> {
    let mut files = parse_with_stats(bytes, true, false)?;
    for file in &mut files {
        if file.summary.is_none() {
            file.added = Some(
                file.hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.text.starts_with('+'))
                    .count(),
            );
            file.deleted = Some(
                file.hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.text.starts_with('-'))
                    .count(),
            );
        }
    }
    Ok(files)
}

fn parse_with_stats(
    mut bytes: &[u8],
    summarize_non_utf8: bool,
    stats: bool,
) -> Result<Vec<FileDiff>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    while bytes.first() == Some(&b':') {
        let raw = utf8(nul(&mut bytes)?)?;
        let fields: Vec<_> = raw[1..].split(' ').collect();
        if fields.len() != 5
            || !crate::git::valid_oid(fields[2])
            || !crate::git::valid_oid(fields[3])
            || !fields[..2].iter().all(|mode| {
                mode.len() == 6 && mode.bytes().all(|byte| (b'0'..=b'7').contains(&byte))
            })
        {
            return Err("Unsupported combined or malformed Git diff".into());
        }
        let status = fields[4].chars().next().ok_or("Missing diff status")?;
        if !matches!(status, 'A' | 'D' | 'M' | 'R' | 'C' | 'T') {
            return Err(format!(
                "Unsupported Git diff status {status}; resolve conflicts before viewing"
            ));
        }
        let old_path = path(nul(&mut bytes)?)?;
        let new_path = if matches!(status, 'R' | 'C') {
            path(nul(&mut bytes)?)?
        } else {
            old_path.clone()
        };
        if files.len() >= 4096 {
            return Err("Diff exceeds the 4096 file limit".into());
        }
        files.push(FileDiff {
            old_path,
            new_path,
            old_oid: fields[2].into(),
            new_oid: fields[3].into(),
            status,
            old_mode: fields[0].into(),
            new_mode: fields[1].into(),
            added: None,
            deleted: None,
            hunks: Vec::new(),
            summary: None,
            patch_header: String::new(),
        });
    }
    for file in files.iter_mut().filter(|_| stats) {
        let stat = nul(&mut bytes)?;
        let mut fields = stat.splitn(3, |byte| *byte == b'\t');
        let added = fields.next().ok_or("Missing diffstat")?;
        let deleted = fields.next().ok_or("Missing diffstat")?;
        let name = fields.next().ok_or("Missing diffstat path")?;
        let number = |bytes: &[u8]| -> Result<Option<usize>, String> {
            if bytes == b"-" {
                Ok(None)
            } else {
                Ok(Some(
                    utf8(bytes)?.parse().map_err(|_| "Invalid diffstat count")?,
                ))
            }
        };
        file.added = number(added)?;
        file.deleted = number(deleted)?;
        if name.is_empty() {
            if path(nul(&mut bytes)?)? != file.old_path || path(nul(&mut bytes)?)? != file.new_path
            {
                return Err("Diffstat path mismatch".into());
            }
        } else if path(name)? != file.new_path {
            return Err("Diffstat path mismatch".into());
        }
    }
    if bytes.first() != Some(&0) {
        return Err("Missing patch separator".into());
    }
    let patch = &bytes[1..];
    if patch.split(|byte| *byte == b'\n').count() > 100_001 {
        return Err("Diff exceeds the 100000 patch line limit".into());
    }
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in patch.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"diff --git ") && offset != 0 {
            blocks.push(&patch[start..offset]);
            start = offset;
        }
        offset += line.len();
    }
    if !patch.is_empty() {
        blocks.push(&patch[start..]);
    }
    let mut blocks = blocks.into_iter();
    for file in &mut files {
        let block = blocks.next().ok_or("Missing file patch")?;
        let addition = if file.status == 'T' {
            Some(
                blocks
                    .next()
                    .ok_or("Missing added side of file-type change")?,
            )
        } else {
            None
        };
        if summarize_non_utf8
            && (utf8(block).is_err() || addition.is_some_and(|block| utf8(block).is_err()))
        {
            file.summary = Some("Non-UTF-8 contents differ (text hunks unavailable)".into());
            continue;
        }
        let block = utf8(block)?;
        if let Some(addition) = addition {
            let addition = utf8(addition)?;
            let deleted_mode = format!("deleted file mode {}", file.old_mode);
            let added_mode = format!("new file mode {}", file.new_mode);
            if !block.lines().any(|line| line == deleted_mode)
                || !addition.lines().any(|line| line == added_mode)
            {
                return Err("Invalid split file-type patch".into());
            }
            let mut old = file.clone();
            old.status = 'D';
            old.new_oid = "0".repeat(old.old_oid.len());
            old.new_mode = "000000".into();
            parse_file(&mut old, block)?;
            if !old.new_oid.bytes().all(|byte| byte == b'0') {
                return Err("Invalid deleted blob in file-type patch".into());
            }
            let mut new = file.clone();
            new.status = 'A';
            new.old_oid = "0".repeat(new.new_oid.len());
            new.old_mode = "000000".into();
            parse_file(&mut new, addition)?;
            file.new_oid = new.new_oid;
            file.summary = Some("File-type change (text navigation unavailable)".into());
        } else {
            parse_file(file, block)?;
        }
    }
    if blocks.next().is_some() {
        return Err("Unexpected extra file patch".into());
    }
    Ok(files)
}

pub(crate) fn range(value: &str, prefix: char) -> Result<(usize, usize), String> {
    let value = value.strip_prefix(prefix).ok_or("Malformed hunk range")?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    let number = |value: &str| {
        value
            .parse::<usize>()
            .map_err(|_| "Malformed hunk number".to_owned())
    };
    let start = number(start)?;
    let count = number(count)?;
    if start.checked_add(count).is_none() || (start == 0 && count != 0) {
        return Err("Invalid hunk range".into());
    }
    Ok((start, count))
}

fn parse_file(file: &mut FileDiff, block: &str) -> Result<(), String> {
    file.patch_header = block
        .split_inclusive('\n')
        .take_while(|line| !line.starts_with("@@ "))
        .collect();
    let mut lines = block.split_inclusive('\n').peekable();
    if !lines
        .next()
        .is_some_and(|line| line.starts_with("diff --git "))
    {
        return Err("Missing file patch header".into());
    }
    while let Some(line) = lines.next() {
        if let Some(index) = line.strip_prefix("index ") {
            let ids = index
                .split_whitespace()
                .next()
                .ok_or("Missing blob identities")?;
            let (old, new) = ids.split_once("..").ok_or("Malformed blob identities")?;
            if !crate::git::valid_oid(old)
                || !crate::git::valid_oid(new)
                || old != file.old_oid
                || (!file.new_oid.bytes().all(|byte| byte == b'0') && file.new_oid != new)
            {
                return Err("Patch version mismatch".into());
            }
            file.new_oid = new.into();
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            file.summary = Some("Binary contents differ (text navigation unavailable)".into());
        } else if line.starts_with("@@ ") {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 4 || fields[3] != "@@" {
                return Err("Malformed unified hunk".into());
            }
            let (mut old, old_count) = range(fields[1], '-')?;
            let (mut new, new_count) = range(fields[2], '+')?;
            let mut hunk = Hunk {
                header: line.trim_end_matches('\n').into(),
                old_start: old,
                new_start: new,
                lines: Vec::new(),
            };
            let (mut old_left, mut new_left) = (old_count, new_count);
            while let Some(line) = lines.peek().copied() {
                if line.starts_with("\\ No newline at end of file") {
                    lines.next();
                    hunk.lines.push(DiffLine {
                        text: line.into(),
                        old_line: None,
                        new_line: None,
                    });
                    continue;
                }
                if old_left == 0 && new_left == 0 {
                    break;
                }
                lines.next();
                let kind = line.as_bytes().first().copied().ok_or("Empty patch line")?;
                let (old_line, new_line) = match kind {
                    b' ' if old_left > 0 && new_left > 0 => {
                        old_left -= 1;
                        new_left -= 1;
                        let positions = (Some(old), Some(new));
                        old += 1;
                        new += 1;
                        positions
                    }
                    b'-' if old_left > 0 => {
                        old_left -= 1;
                        let position = old;
                        old += 1;
                        (Some(position), None)
                    }
                    b'+' if new_left > 0 => {
                        new_left -= 1;
                        let position = new;
                        new += 1;
                        (None, Some(position))
                    }
                    _ => return Err("Malformed patch body or unsupported diff".into()),
                };
                hunk.lines.push(DiffLine {
                    text: line.into(),
                    old_line,
                    new_line,
                });
            }
            if old_left != 0 || new_left != 0 {
                return Err("Truncated hunk body".into());
            }
            file.hunks.push(hunk);
        }
    }
    if file.old_mode == "160000" || file.new_mode == "160000" {
        file.hunks.clear();
        file.summary = Some("Submodule change (text navigation unavailable)".into());
    } else if file.old_mode == "120000" || file.new_mode == "120000" || file.status == 'T' {
        file.hunks.clear();
        file.summary = Some("Symlink or file-type change (text navigation unavailable)".into());
    }
    Ok(())
}
