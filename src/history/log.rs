use super::*;

pub(super) fn load(
    repository: &Repository,
    revision: &str,
    limit: usize,
    environment: ReadEnvironment,
    cancelled: &AtomicBool,
) -> Result<Log, String> {
    let head = environment.execute(
        repository,
        &[
            "rev-parse".as_ref(),
            "--verify".as_ref(),
            "--quiet".as_ref(),
            "HEAD".as_ref(),
        ],
        cancelled,
    )?;
    if head.status.code() == Some(1) {
        return Ok(Log {
            revision: revision.into(),
            entries: Vec::new(),
            more: false,
        });
    }
    if !head.status.success() {
        return Err(format!("Cannot read HEAD: {}", head.diagnostic));
    }
    let bytes = read_environment(
        environment,
        repository,
        &[
            "log",
            "--graph",
            "--decorate=short",
            "--no-color",
            "--no-show-signature",
            "--encoding=UTF-8",
            &format!("-n{limit}"),
            "--format=%x00%H%x00%P%x00%an%x00%aI%x00%s%x00%D%x00",
            "--end-of-options",
            revision,
            "--",
        ],
        cancelled,
    )?;
    let entries = parse(&bytes)?;
    let more = entries
        .iter()
        .filter(|entry| entry.commit.is_some())
        .count()
        == limit;
    Ok(Log {
        revision: revision.into(),
        entries,
        more,
    })
}

pub(super) fn parse(mut bytes: &[u8]) -> Result<Vec<LogEntry>, String> {
    let mut entries = Vec::new();
    while !bytes.is_empty() {
        let end = bytes.iter().position(|byte| *byte == 0);
        let (graph, rest) = match end {
            Some(end) => (&bytes[..end], Some(&bytes[end + 1..])),
            None => (bytes, None),
        };
        let graph = utf8(graph)?;
        let mut lines = graph.split('\n').peekable();
        let prefix = loop {
            let line = lines.next().unwrap_or("");
            if lines.peek().is_none() {
                break line;
            }
            if !line.is_empty() {
                entries.push(LogEntry {
                    graph: line.to_owned(),
                    commit: None,
                });
            }
        };
        let Some(mut rest) = rest else { break };
        let mut field = || -> Result<String, String> {
            let end = rest
                .iter()
                .position(|byte| *byte == 0)
                .ok_or("Truncated log record")?;
            let value = utf8(&rest[..end])?.to_owned();
            rest = &rest[end + 1..];
            Ok(value)
        };
        let oid = field()?;
        let parents = field()?
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !valid_oid(&oid) || parents.iter().any(|oid| !valid_oid(oid)) {
            return Err("Malformed log identity".into());
        }
        let author = field()?;
        let date = field()?;
        let message = field()?;
        let decorations = field()?;
        entries.push(LogEntry {
            graph: prefix.into(),
            commit: Some(CommitInfo {
                oid,
                parents,
                author,
                date,
                message,
                decorations,
            }),
        });
        bytes = rest.strip_prefix(b"\n").unwrap_or(rest);
    }
    Ok(entries)
}
