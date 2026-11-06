use std::collections::HashSet;

use super::{
    Entry, Group, Head, MAX_BYTES, MAX_RECORDS, Record, Rename, Repository, Status, Submodule,
    Tracked,
};
use crate::git::{relative_path, valid_oid};

fn nul<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("Truncated Git status record")?;
    let value = &bytes[..end];
    *bytes = &bytes[end + 1..];
    Ok(value)
}

fn mode(bytes: &[u8]) -> Result<u32, String> {
    if bytes.len() != 6 || !bytes.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
        return Err("Invalid Git status mode".into());
    }
    Ok(bytes
        .iter()
        .fold(0, |value, byte| value * 8 + u32::from(byte - b'0')))
}

fn oid(bytes: &[u8]) -> Result<String, String> {
    let value = std::str::from_utf8(bytes).map_err(|_| "Invalid Git status object ID")?;
    if !valid_oid(value) {
        return Err("Invalid Git status object ID".into());
    }
    Ok(value.into())
}

fn submodule(bytes: &[u8]) -> Result<Submodule, String> {
    if bytes == b"N..." {
        return Ok(Submodule::default());
    }
    if bytes.len() != 4
        || bytes[0] != b'S'
        || !matches!(bytes[1], b'.' | b'C')
        || !matches!(bytes[2], b'.' | b'M')
        || !matches!(bytes[3], b'.' | b'U')
    {
        return Err("Invalid Git status submodule field".into());
    }
    Ok(Submodule {
        is_submodule: true,
        commit_changed: bytes[1] == b'C',
        tracked_dirty: bytes[2] == b'M',
        untracked_dirty: bytes[3] == b'U',
    })
}

fn fields(record: &[u8], count: usize) -> Result<Vec<&[u8]>, String> {
    let fields: Vec<_> = record.splitn(count, |byte| *byte == b' ').collect();
    if fields.len() != count || fields.iter().any(|value| value.is_empty()) {
        return Err("Malformed Git status fields".into());
    }
    Ok(fields)
}

fn tracked(fields: &[&[u8]]) -> Result<Tracked, String> {
    Ok(Tracked {
        head_mode: mode(fields[3])?,
        index_mode: mode(fields[4])?,
        worktree_mode: mode(fields[5])?,
        head_oid: oid(fields[6])?,
        index_oid: oid(fields[7])?,
    })
}

fn xy(value: &[u8], kind: u8) -> Result<[u8; 2], String> {
    let value: [u8; 2] = value.try_into().map_err(|_| "Invalid Git status XY")?;
    let conflict = matches!(
        value,
        [b'D', b'D']
            | [b'A', b'U']
            | [b'U', b'D']
            | [b'U', b'A']
            | [b'D', b'U']
            | [b'A', b'A']
            | [b'U', b'U']
    );
    let valid = if kind == b'u' {
        conflict
    } else {
        !conflict
            && value != *b".."
            && value
                .iter()
                .all(|byte| matches!(byte, b'.' | b'M' | b'A' | b'D' | b'T' | b'R' | b'C'))
    };
    if !valid {
        return Err("Invalid Git status XY for its record type".into());
    }
    Ok(value)
}

fn entry(record: &[u8], bytes: &mut &[u8]) -> Result<Entry, String> {
    let kind = *record.first().ok_or("Empty Git status record")?;
    if kind == b'?' {
        let path = record
            .strip_prefix(b"? ")
            .ok_or("Malformed untracked record")?;
        let directory = path.ends_with(b"/");
        return Ok(Entry {
            path: relative_path(if directory {
                &path[..path.len() - 1]
            } else {
                path
            })?,
            xy: *b"??",
            submodule: Submodule::default(),
            record: Record::Untracked { directory },
        });
    }
    let count = match kind {
        b'1' => 9,
        b'2' => 10,
        b'u' => 11,
        _ => return Err("Unsupported Git status record type".into()),
    };
    let fields = fields(record, count)?;
    if fields[0] != [kind] {
        return Err("Invalid Git status record or XY".into());
    }
    let xy = xy(fields[1], kind)?;
    let submodule = submodule(fields[2])?;
    let record = if kind == b'u' {
        Record::Unmerged {
            modes: [
                mode(fields[3])?,
                mode(fields[4])?,
                mode(fields[5])?,
                mode(fields[6])?,
            ],
            oids: [oid(fields[7])?, oid(fields[8])?, oid(fields[9])?],
        }
    } else {
        let rename = if kind == b'2' {
            let score = fields[8];
            if score.len() < 2
                || !matches!(score[0], b'R' | b'C')
                || !xy.contains(&score[0])
                || !score[1..].iter().all(u8::is_ascii_digit)
            {
                return Err("Invalid Git rename score".into());
            }
            let percentage: u8 = std::str::from_utf8(&score[1..])
                .unwrap()
                .parse()
                .map_err(|_| "Invalid Git rename score")?;
            if percentage > 100 {
                return Err("Invalid Git rename score".into());
            }
            Some(Rename {
                source: relative_path(nul(bytes)?)?,
                copy: score[0] == b'C',
                score: percentage,
            })
        } else {
            if xy.iter().any(|byte| matches!(byte, b'R' | b'C')) {
                return Err("Rename is missing its source record".into());
            }
            None
        };
        Record::Tracked {
            state: tracked(&fields)?,
            rename,
        }
    };
    Ok(Entry {
        path: relative_path(fields[count - 1])?,
        xy,
        submodule,
        record,
    })
}

pub(super) fn parse(repository: Repository, mut bytes: &[u8]) -> Result<Status, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Status exceeds the 16 MiB input limit".into());
    }
    let mut head_oid = None;
    let mut branch = None;
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    while !bytes.is_empty() {
        let record = nul(&mut bytes)?;
        if let Some(header) = record.strip_prefix(b"# ") {
            let end = header
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or("Malformed Git status header")?;
            let (name, value) = (&header[..end], &header[end + 1..]);
            if value.is_empty() {
                return Err("Empty Git status header value".into());
            }
            match name {
                b"branch.oid" => {
                    if head_oid.is_some() {
                        return Err("Duplicate Git HEAD header".into());
                    }
                    head_oid = Some(if value == b"(initial)" {
                        None
                    } else {
                        let value = oid(value)?;
                        if value.bytes().all(|byte| byte == b'0') {
                            return Err("Invalid zero Git HEAD object ID".into());
                        }
                        Some(value)
                    });
                }
                b"branch.head" if branch.replace(value.to_vec()).is_some() => {
                    return Err("Duplicate Git branch header".into());
                }
                _ => {}
            }
        } else {
            if entries.len() >= MAX_RECORDS {
                return Err("Status exceeds the 4096 record limit".into());
            }
            let entry = entry(record, &mut bytes)?;
            for group in Group::ALL {
                if entry.in_group(group) && !paths.insert((group, entry.path.clone())) {
                    return Err("Duplicate Git status path in a group".into());
                }
            }
            entries.push(entry);
        }
    }
    let oid = head_oid.ok_or("Missing Git HEAD header")?;
    let name = branch.ok_or("Missing Git branch header")?;
    let head = match (name.as_slice(), oid) {
        (b"(detached)", Some(oid)) => Head::Detached { oid },
        (b"(detached)", None) => return Err("Detached HEAD cannot be unborn".into()),
        (_, Some(oid)) => Head::Branch { name, oid },
        (_, None) => Head::Unborn { name },
    };
    let mut oid_length = match &head {
        Head::Branch { oid, .. } | Head::Detached { oid } => Some(oid.len()),
        Head::Unborn { .. } => None,
    };
    for entry in &entries {
        let ids: &[String] = match &entry.record {
            Record::Tracked { state, .. } => std::slice::from_ref(&state.head_oid),
            Record::Unmerged { oids, .. } => oids,
            Record::Untracked { .. } => &[],
        };
        for id in ids.iter().chain(match &entry.record {
            Record::Tracked { state, .. } => Some(&state.index_oid),
            _ => None,
        }) {
            if *oid_length.get_or_insert(id.len()) != id.len() {
                return Err("Mixed Git object ID lengths".into());
            }
        }
    }
    Ok(Status {
        repository,
        head,
        entries,
    })
}
