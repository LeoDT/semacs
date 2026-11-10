use super::{Blob, utf8};
use crate::git::{relative_path, valid_oid};

#[derive(Clone, Debug)]
pub(crate) struct File {
    pub parents: Vec<Blob>,
    pub result: Blob,
    pub body: Body,
}

#[derive(Clone, Debug)]
pub(crate) enum Body {
    Hunks(Vec<Hunk>),
    Omitted,
    Summary(String),
}

impl File {
    pub fn hunks(&self) -> &[Hunk] {
        match &self.body {
            Body::Hunks(hunks) => hunks,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Hunk {
    pub header: String,
    pub parents: Vec<usize>,
    pub result: usize,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub text: String,
    pub parents: Vec<usize>,
    pub result: usize,
}

fn nul<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("Truncated combined metadata")?;
    let field = &bytes[..end];
    *bytes = &bytes[end + 1..];
    Ok(field)
}

pub(super) fn parse(mut bytes: &[u8], count: usize) -> Result<Vec<File>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if count < 2 {
        return Err("Combined diff requires multiple parents".into());
    }
    let mut files = Vec::new();
    while bytes.first() == Some(&b':') {
        let header = utf8(nul(&mut bytes)?)?;
        let columns = header.bytes().take_while(|byte| *byte == b':').count();
        let fields = header[columns..].split(' ').collect::<Vec<_>>();
        if columns != count || fields.len() != (count + 1) * 2 + 1 {
            return Err("Malformed combined raw record".into());
        }
        let modes = &fields[..=count];
        let oids = &fields[count + 1..(count + 1) * 2];
        let status = fields.last().unwrap();
        if modes
            .iter()
            .any(|mode| mode.len() != 6 || !mode.bytes().all(|byte| (b'0'..=b'7').contains(&byte)))
            || oids.iter().any(|oid| !valid_oid(oid))
            || status.len() != count
            || !status.bytes().all(|byte| b"ACDMRT".contains(&byte))
        {
            return Err("Invalid combined file identities".into());
        }
        let mut versions = Vec::with_capacity(count + 1);
        for index in 0..=count {
            versions.push(Blob {
                path: relative_path(nul(&mut bytes)?)?,
                mode: modes[index].into(),
                oid: oids[index].into(),
            });
        }
        let result = versions.pop().unwrap();
        files.push(File {
            parents: versions,
            result,
            body: Body::Omitted,
        });
        if files.len() > 4096 {
            return Err("Diff exceeds the 4096 file limit".into());
        }
    }
    if bytes.first() != Some(&0) {
        return Err("Missing combined patch separator".into());
    }
    let bytes = &bytes[1..];
    if bytes.split(|byte| *byte == b'\n').count() > 100_001 {
        return Err("Diff exceeds the 100000 patch line limit".into());
    }
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if (line.starts_with(b"diff --cc ") || line.starts_with(b"diff --combined ")) && offset != 0
        {
            blocks.push(&bytes[start..offset]);
            start = offset;
        }
        offset += line.len();
    }
    if !bytes.is_empty() {
        blocks.push(&bytes[start..]);
    }
    let mut paths = std::collections::BTreeMap::new();
    for (index, file) in files.iter_mut().enumerate() {
        if paths.insert(file.result.path.clone(), index).is_some() {
            return Err("Duplicate combined file identity".into());
        }
        if file
            .parents
            .iter()
            .chain([&file.result])
            .any(|blob| !matches!(blob.mode.as_str(), "000000" | "100644" | "100755"))
        {
            file.body = Body::Summary(
                "Submodule, symlink or file-type change (text navigation unavailable)".into(),
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    for block in blocks {
        let path = patch_path(block)?;
        let index = *paths
            .get(&path)
            .ok_or("Combined patch has no matching raw file")?;
        if !seen.insert(index) {
            return Err("Duplicate combined file patch".into());
        }
        let file = &mut files[index];
        if matches!(file.body, Body::Summary(_)) {
            continue;
        }
        let Ok(block) = utf8(block) else {
            file.body = Body::Summary("Non-UTF-8 contents differ (text hunks unavailable)".into());
            continue;
        };
        if block
            .lines()
            .any(|line| line.starts_with("Binary files ") || line.starts_with("GIT binary patch"))
        {
            file.body =
                Body::Summary("Binary contents differ (text navigation unavailable)".into());
            continue;
        }
        file.body = Body::Hunks(hunks(block, count)?);
    }
    Ok(files)
}

fn patch_path(block: &[u8]) -> Result<std::path::PathBuf, String> {
    let heading = block
        .split(|byte| *byte == b'\n')
        .next()
        .ok_or("Missing combined file heading")?;
    let path = heading
        .strip_prefix(b"diff --cc ")
        .or_else(|| heading.strip_prefix(b"diff --combined "))
        .ok_or("Missing combined file heading")?;
    if !path.starts_with(b"\"") {
        return relative_path(path);
    }
    let mut quoted = path
        .strip_prefix(b"\"")
        .and_then(|path| path.strip_suffix(b"\""))
        .ok_or("Malformed quoted Git path")?;
    let mut decoded = Vec::with_capacity(quoted.len());
    while let Some((&byte, rest)) = quoted.split_first() {
        quoted = rest;
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        let (&escaped, rest) = quoted.split_first().ok_or("Truncated Git path escape")?;
        quoted = rest;
        let byte = match escaped {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'\\' => b'\\',
            b'"' => b'"',
            b'0'..=b'3' => {
                let tail = quoted
                    .get(..2)
                    .filter(|tail| tail.iter().all(|byte| (b'0'..=b'7').contains(byte)))
                    .ok_or("Invalid Git octal path escape")?;
                let byte = (escaped - b'0') * 64 + (tail[0] - b'0') * 8 + (tail[1] - b'0');
                quoted = &quoted[2..];
                byte
            }
            _ => return Err("Invalid Git path escape".into()),
        };
        decoded.push(byte);
    }
    relative_path(&decoded)
}

fn hunks(block: &str, count: usize) -> Result<Vec<Hunk>, String> {
    let marker = "@".repeat(count + 1);
    let mut lines = block.split_inclusive('\n').peekable();
    let mut hunks = Vec::new();
    while let Some(line) = lines.next() {
        if !line.starts_with('@') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < count + 3 || fields[0] != marker || fields[count + 2] != marker {
            return Err("Malformed combined hunk".into());
        }
        let mut positions = Vec::with_capacity(count + 1);
        let mut remaining = Vec::with_capacity(count + 1);
        for (index, field) in fields[1..count + 2].iter().enumerate() {
            let (start, size) =
                crate::diff::parse::range(field, if index == count { '+' } else { '-' })?;
            positions.push(start);
            remaining.push(size);
        }
        let mut hunk = Hunk {
            header: line.trim_end_matches('\n').into(),
            parents: positions[..count].to_vec(),
            result: positions[count],
            lines: Vec::new(),
        };
        while let Some(line) = lines.peek().copied() {
            if line.starts_with("\\ No newline at end of file") {
                lines.next();
                hunk.lines.push(Line {
                    text: line.into(),
                    parents: positions[..count].to_vec(),
                    result: positions[count],
                });
                continue;
            }
            if remaining.iter().all(|size| *size == 0) {
                break;
            }
            lines.next();
            let prefix = line
                .as_bytes()
                .get(..count)
                .ok_or("Truncated combined prefix")?;
            if !prefix.iter().all(|byte| b" +-".contains(byte))
                || (prefix.contains(&b'-') && prefix.contains(&b'+'))
            {
                return Err("Invalid combined line prefix".into());
            }
            let removed = prefix.contains(&b'-');
            let mapped = Line {
                text: line.into(),
                parents: positions[..count].to_vec(),
                result: positions[count],
            };
            for index in 0..=count {
                let consumed = if index == count {
                    !removed
                } else if removed {
                    prefix[index] == b'-'
                } else {
                    prefix[index] != b'+'
                };
                if consumed {
                    remaining[index] = remaining[index]
                        .checked_sub(1)
                        .ok_or("Combined line exceeds hunk range")?;
                    positions[index] += 1;
                }
            }
            hunk.lines.push(mapped);
        }
        if remaining.iter().any(|size| *size != 0) {
            return Err("Truncated combined hunk".into());
        }
        hunks.push(hunk);
    }
    Ok(hunks)
}
