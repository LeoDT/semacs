use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const SYSTEMD_TIMEOUT: Duration = Duration::from_millis(500);
const SHELL_TIMEOUT: Duration = Duration::from_millis(1500);
const ENV_OUTPUT_LIMIT: u64 = 256 * 1024;
const AGENT_TIMEOUT: Duration = Duration::from_millis(100);
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENT_FAILURE: u8 = 5;
const SHELL_ENV_BEGIN: &[u8] = b"__SEMACS_ENV_BEGIN__";
const SHELL_ENV_END: &[u8] = b"__SEMACS_ENV_END__";
const SHELL_ENV_CMD: &str = "printf '__SEMACS_ENV_BEGIN__'; env -0; printf '__SEMACS_ENV_END__'";

/// Resolves a fallback `SSH_AUTH_SOCK` path when the inherited environment variable
/// is missing, empty, or points to an unreachable socket.
///
/// Returns `None` when the inherited `SSH_AUTH_SOCK` is already connectable (so the
/// caller preserves it untouched) or when no responsive user-owned SSH agent socket
/// can be found.
pub(super) fn resolve_ssh_auth_sock() -> Option<PathBuf> {
    let inherited = std::env::var_os("SSH_AUTH_SOCK");
    let euid = unsafe { libc::geteuid() as u32 };
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{euid}")));
    let home_dir = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let mut temp_dirs = vec![std::env::temp_dir()];
    let tmp = PathBuf::from("/tmp");
    if temp_dirs[0] != tmp {
        temp_dirs.push(tmp);
    }

    let hostname = current_hostname();

    resolve_with(
        inherited.as_deref(),
        systemd_ssh_auth_sock,
        Some(&runtime_dir),
        home_dir.as_deref(),
        hostname.as_deref(),
        &temp_dirs,
        shell_ssh_auth_sock,
        euid,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_with(
    inherited: Option<&OsStr>,
    systemd_lookup: impl FnOnce() -> Option<PathBuf>,
    runtime_dir: Option<&Path>,
    home_dir: Option<&Path>,
    hostname: Option<&str>,
    temp_dirs: &[PathBuf],
    shell_lookup: impl FnOnce() -> Option<PathBuf>,
    euid: u32,
) -> Option<PathBuf> {
    if let Some(value) = inherited.filter(|value| !value.is_empty())
        && UnixStream::connect(Path::new(value)).is_ok()
    {
        return None;
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut push_candidate = |path: PathBuf| {
        if !path.as_os_str().is_empty() && seen.insert(path.clone()) {
            candidates.push(path);
        }
    };

    if let Some(path) = systemd_lookup() {
        push_candidate(path);
    }

    if let Some(runtime_dir) = runtime_dir {
        for relative in [
            "ssh-agent.socket",
            "ssh-agent.sock",
            "openssh_agent",
            "gcr/ssh",
            "keyring/ssh",
            "gnupg/S.gpg-agent.ssh",
        ] {
            push_candidate(runtime_dir.join(relative));
        }
    }

    if let Some(home_dir) = home_dir {
        for path in discover_keychain_sockets(home_dir, hostname, euid) {
            push_candidate(path);
        }
        for relative in [".1password/agent.sock", ".bitwarden-ssh-agent.sock"] {
            push_candidate(home_dir.join(relative));
        }
        for path in discover_home_agent_sockets(home_dir, euid) {
            push_candidate(path);
        }
    }

    for path in discover_temp_agent_sockets(temp_dirs, euid) {
        push_candidate(path);
    }

    let mut empty_fallback = None;
    for candidate in candidates {
        match probe_ssh_agent(&candidate, euid) {
            Some(nkeys) if nkeys > 0 => return Some(candidate),
            Some(0) if empty_fallback.is_none() => {
                empty_fallback = Some(candidate);
            }
            _ => {}
        }
    }

    if let Some(path) = shell_lookup()
        && !path.as_os_str().is_empty()
        && seen.insert(path.clone())
    {
        match probe_ssh_agent(&path, euid) {
            Some(nkeys) if nkeys > 0 => return Some(path),
            Some(0) if empty_fallback.is_none() => {
                empty_fallback = Some(path);
            }
            _ => {}
        }
    }

    empty_fallback
}

fn run_bounded_stdout(mut command: Command, timeout: Duration, limit: u64) -> Option<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(limit).read_to_end(&mut bytes);
        bytes
    });

    let start = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    unsafe {
                        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };

    let output = reader.join().ok()?;
    success.then_some(output)
}

fn systemd_ssh_auth_sock() -> Option<PathBuf> {
    let mut command = Command::new("systemctl");
    command
        .args(["--user", "show-environment"])
        .process_group(0);
    let output = run_bounded_stdout(command, SYSTEMD_TIMEOUT, ENV_OUTPUT_LIMIT)?;
    parse_systemd_ssh_auth_sock(&output)
}

fn shell_ssh_auth_sock() -> Option<PathBuf> {
    if std::env::var_os("HOME").is_none_or(|home| home.is_empty()) {
        return None;
    }
    let shell = PathBuf::from(std::env::var_os("SHELL").filter(|value| !value.is_empty())?);
    if !shell.is_absolute() {
        return None;
    }
    let name = shell.file_name()?.to_str()?;
    if !matches!(
        name,
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "ash" | "mksh"
    ) {
        return None;
    }

    let mut command = Command::new(&shell);
    command
        .args(["-l", "-i", "-c", SHELL_ENV_CMD])
        .process_group(0);
    let output = run_bounded_stdout(command, SHELL_TIMEOUT, ENV_OUTPUT_LIMIT)?;
    parse_shell_env_ssh_auth_sock(&output)
}

fn parse_shell_env_ssh_auth_sock(bytes: &[u8]) -> Option<PathBuf> {
    let start = bytes
        .windows(SHELL_ENV_BEGIN.len())
        .position(|window| window == SHELL_ENV_BEGIN)?
        + SHELL_ENV_BEGIN.len();
    let end = bytes[start..]
        .windows(SHELL_ENV_END.len())
        .position(|window| window == SHELL_ENV_END)?;
    for record in bytes[start..start + end].split(|byte| *byte == 0) {
        let Some(value) = record.strip_prefix(b"SSH_AUTH_SOCK=") else {
            continue;
        };
        if value.is_empty() {
            return None;
        }
        return Some(PathBuf::from(OsString::from_vec(value.to_vec())));
    }
    None
}

fn parse_systemd_ssh_auth_sock(bytes: &[u8]) -> Option<PathBuf> {
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(raw) = line.strip_prefix(b"SSH_AUTH_SOCK=") else {
            continue;
        };
        let value = strip_quotes(raw);
        if value.is_empty() {
            return None;
        }
        return Some(PathBuf::from(OsString::from_vec(value.to_vec())));
    }
    None
}

fn strip_quotes(value: &[u8]) -> &[u8] {
    if value.len() >= 2
        && ((value.starts_with(b"'") && value.ends_with(b"'"))
            || (value.starts_with(b"\"") && value.ends_with(b"\"")))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn current_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let len = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    let host = String::from_utf8_lossy(&buf[..len])
        .trim()
        .trim_end_matches('.')
        .to_owned();
    (!host.is_empty()).then_some(host)
}

fn discover_keychain_sockets(home_dir: &Path, hostname: Option<&str>, euid: u32) -> Vec<PathBuf> {
    let Some(hostname) = hostname.filter(|name| !name.is_empty()) else {
        return Vec::new();
    };
    let keychain_dir = home_dir.join(".keychain");
    let Ok(dir_meta) = std::fs::symlink_metadata(&keychain_dir) else {
        return Vec::new();
    };
    if !dir_meta.file_type().is_dir() || dir_meta.uid() != euid {
        return Vec::new();
    }

    let mut hosts = vec![hostname];
    if let Some((short, _)) = hostname.split_once('.')
        && !short.is_empty()
    {
        hosts.push(short);
    }

    let mut found = Vec::new();
    for host in hosts {
        for suffix in ["-sh", "-fish"] {
            let file_path = keychain_dir.join(format!("{host}{suffix}"));
            let Ok(meta) = std::fs::symlink_metadata(&file_path) else {
                continue;
            };
            if !meta.file_type().is_file() || meta.uid() != euid {
                continue;
            }
            let Ok(file) = std::fs::File::open(&file_path) else {
                continue;
            };
            let mut bytes = Vec::new();
            if file.take(16 * 1024).read_to_end(&mut bytes).is_err() {
                continue;
            }
            if let Some(sock) = parse_keychain_ssh_auth_sock(&bytes) {
                found.push(sock);
            }
        }
    }
    found
}

fn parse_keychain_ssh_auth_sock(bytes: &[u8]) -> Option<PathBuf> {
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        for raw_stmt in line.split(|byte| *byte == b';') {
            let stmt = raw_stmt.trim_ascii();
            if let Some(raw) = stmt.strip_prefix(b"SSH_AUTH_SOCK=") {
                let value = strip_quotes(raw.trim_ascii());
                if !value.is_empty() {
                    return Some(PathBuf::from(OsString::from_vec(value.to_vec())));
                }
            } else if stmt.starts_with(b"set ")
                && let Some(value) = parse_fish_set_ssh_auth_sock(stmt)
            {
                return Some(value);
            }
        }
    }
    None
}

fn parse_fish_set_ssh_auth_sock(stmt: &[u8]) -> Option<PathBuf> {
    let mut index = 0;
    let mut saw_var = false;
    while index < stmt.len() {
        while index < stmt.len() && stmt[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= stmt.len() {
            break;
        }
        if saw_var {
            let rest = &stmt[index..];
            let value = if let Some(&quote) = rest.first().filter(|b| matches!(b, b'\'' | b'"')) {
                let tail = &rest[1..];
                let end = tail.iter().position(|b| *b == quote)?;
                &tail[..end]
            } else {
                let end = rest
                    .iter()
                    .position(u8::is_ascii_whitespace)
                    .unwrap_or(rest.len());
                &rest[..end]
            };
            if value.is_empty() {
                return None;
            }
            return Some(PathBuf::from(OsString::from_vec(value.to_vec())));
        }
        let end = stmt[index..]
            .iter()
            .position(u8::is_ascii_whitespace)
            .map_or(stmt.len(), |offset| index + offset);
        let token = &stmt[index..end];
        if token == b"SSH_AUTH_SOCK" {
            saw_var = true;
        }
        index = end;
    }
    None
}

fn discover_home_agent_sockets(home_dir: &Path, euid: u32) -> Vec<PathBuf> {
    let agent_dir = home_dir.join(".ssh/agent");
    let Ok(dir_meta) = std::fs::symlink_metadata(&agent_dir) else {
        return Vec::new();
    };
    if !dir_meta.file_type().is_dir()
        || dir_meta.uid() != euid
        || (dir_meta.permissions().mode() & 0o077) != 0
    {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&agent_dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let sock_path = entry.path();
        let Ok(sock_meta) = std::fs::symlink_metadata(&sock_path) else {
            continue;
        };
        if !sock_meta.file_type().is_socket() || sock_meta.uid() != euid {
            continue;
        }
        let modified = sock_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        found.push((modified, sock_path));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

fn discover_temp_agent_sockets(temp_dirs: &[PathBuf], euid: u32) -> Vec<PathBuf> {
    let mut visited_dirs = HashSet::new();
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();

    for base in temp_dirs {
        let canonical = base.canonicalize().unwrap_or_else(|_| base.clone());
        if !visited_dirs.insert(canonical) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("ssh-") {
                continue;
            }
            let dir_path = entry.path();
            let Ok(dir_meta) = std::fs::symlink_metadata(&dir_path) else {
                continue;
            };
            if !dir_meta.file_type().is_dir()
                || dir_meta.uid() != euid
                || (dir_meta.permissions().mode() & 0o077) != 0
            {
                continue;
            }
            let Ok(children) = std::fs::read_dir(&dir_path) else {
                continue;
            };
            for child in children.flatten() {
                if !child.file_name().to_string_lossy().starts_with("agent.") {
                    continue;
                }
                let sock_path = child.path();
                let Ok(sock_meta) = std::fs::symlink_metadata(&sock_path) else {
                    continue;
                };
                if !sock_meta.file_type().is_socket() || sock_meta.uid() != euid {
                    continue;
                }
                let modified = sock_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                found.push((modified, sock_path));
            }
        }
    }

    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

fn probe_ssh_agent(path: &Path, euid: u32) -> Option<u32> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.file_type().is_socket() || meta.uid() != euid {
        return None;
    }
    let mut stream = UnixStream::connect(path).ok()?;
    stream.set_read_timeout(Some(AGENT_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(AGENT_TIMEOUT)).ok()?;

    const REQUEST_IDENTITIES: [u8; 5] = [0, 0, 0, 1, SSH_AGENTC_REQUEST_IDENTITIES];
    stream.write_all(&REQUEST_IDENTITIES).ok()?;

    let mut header = [0u8; 5];
    stream.read_exact(&mut header).ok()?;
    let length = u32::from_be_bytes(header[..4].try_into().unwrap());
    match header[4] {
        SSH_AGENT_IDENTITIES_ANSWER if length >= 5 => {
            let mut count = [0u8; 4];
            stream.read_exact(&mut count).ok()?;
            Some(u32::from_be_bytes(count))
        }
        SSH_AGENT_FAILURE => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "semacs-ssh-agent-{}-{}-{name}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn spawn_mock_agent(path: &Path, reply: Vec<u8>) -> std::thread::JoinHandle<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut req = [0u8; 5];
                let _ = stream.read_exact(&mut req);
                let _ = stream.write_all(&reply);
            }
        })
    }

    fn identities_reply(nkeys: u32) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&5u32.to_be_bytes());
        packet.push(SSH_AGENT_IDENTITIES_ANSWER);
        packet.extend_from_slice(&nkeys.to_be_bytes());
        packet
    }

    #[test]
    fn preserves_valid_inherited_ssh_auth_sock() {
        let dir = TempDir::new("preserve");
        let inherited = dir.0.join("inherited.sock");
        let fallback = dir.0.join("ssh-agent.socket");
        let _inherited_handle = spawn_mock_agent(&inherited, identities_reply(0));
        let _fallback_listener = UnixListener::bind(&fallback).unwrap();
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(
            Some(inherited.as_os_str()),
            || Some(fallback.clone()),
            Some(&dir.0),
            None,
            None,
            &[],
            || None,
            euid,
        );
        assert_eq!(resolved, None);
    }

    #[test]
    fn falls_back_when_inherited_sock_is_missing_or_stale() {
        let dir = TempDir::new("stale");
        let stale = dir.0.join("missing.sock");
        let fallback = dir.0.join("ssh-agent.socket");
        let handle = spawn_mock_agent(&fallback, identities_reply(1));
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(
            Some(stale.as_os_str()),
            || None,
            Some(&dir.0),
            None,
            None,
            &[],
            || None,
            euid,
        );
        handle.join().unwrap();
        assert_eq!(resolved.as_deref(), Some(fallback.as_path()));
    }

    #[test]
    fn prefers_agent_with_loaded_keys_over_empty_higher_tier_agent() {
        let dir = TempDir::new("prefer-keys");
        let runtime_dir = dir.0.join("run");
        let temp_root = dir.0.join("tmp");
        let empty_runtime = runtime_dir.join("ssh-agent.socket");
        let ssh_dir = temp_root.join("ssh-1234");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        std::fs::set_permissions(&ssh_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let loaded_tmp = ssh_dir.join("agent.100");

        let h1 = spawn_mock_agent(&empty_runtime, identities_reply(0));
        let h2 = spawn_mock_agent(&loaded_tmp, identities_reply(2));
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(
            None,
            || None,
            Some(&runtime_dir),
            None,
            None,
            &[temp_root],
            || None,
            euid,
        );
        h1.join().unwrap();
        h2.join().unwrap();
        assert_eq!(resolved.as_deref(), Some(loaded_tmp.as_path()));
    }

    #[test]
    fn falls_back_to_first_responsive_empty_agent_when_none_have_keys() {
        let dir = TempDir::new("empty-fallback");
        let runtime_dir = dir.0.join("run");
        let systemd_sock = dir.0.join("systemd.sock");
        let runtime_sock = runtime_dir.join("gcr/ssh");
        let h1 = spawn_mock_agent(&systemd_sock, identities_reply(0));
        let h2 = spawn_mock_agent(&runtime_sock, identities_reply(0));
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(
            None,
            || Some(systemd_sock.clone()),
            Some(&runtime_dir),
            None,
            None,
            &[],
            || None,
            euid,
        );
        h1.join().unwrap();
        h2.join().unwrap();
        assert_eq!(resolved.as_deref(), Some(systemd_sock.as_path()));
    }

    #[test]
    fn rejects_non_agent_sockets_and_world_readable_ssh_temp_dirs() {
        let dir = TempDir::new("security");
        let runtime_dir = dir.0.join("run");
        let temp_root = dir.0.join("tmp");
        let bogus_sock = runtime_dir.join("ssh-agent.socket");
        let h1 = spawn_mock_agent(&bogus_sock, b"NOTSSH".to_vec());

        let unsafe_dir = temp_root.join("ssh-unsafe");
        std::fs::create_dir_all(&unsafe_dir).unwrap();
        std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let unsafe_sock = unsafe_dir.join("agent.1");
        let _unsafe_listener = UnixListener::bind(&unsafe_sock).unwrap();
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(
            None,
            || None,
            Some(&runtime_dir),
            None,
            None,
            &[temp_root],
            || None,
            euid,
        );
        h1.join().unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn discovers_openssh_10_home_ssh_agent_sockets_and_rejects_unsafe_mode() {
        let dir = TempDir::new("home-agent");
        let agent_dir = dir.0.join(".ssh/agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::set_permissions(&agent_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home_sock = agent_dir.join("s.abc123.agent.42");
        let handle = spawn_mock_agent(&home_sock, identities_reply(1));
        let euid = unsafe { libc::geteuid() as u32 };

        let resolved = resolve_with(None, || None, None, Some(&dir.0), None, &[], || None, euid);
        handle.join().unwrap();
        assert_eq!(resolved.as_deref(), Some(home_sock.as_path()));

        let unsafe_dir = TempDir::new("home-agent-unsafe");
        let unsafe_agent_dir = unsafe_dir.0.join(".ssh/agent");
        std::fs::create_dir_all(&unsafe_agent_dir).unwrap();
        std::fs::set_permissions(&unsafe_agent_dir, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let unsafe_sock = unsafe_agent_dir.join("s.unsafe");
        let _listener = UnixListener::bind(&unsafe_sock).unwrap();

        let resolved = resolve_with(
            None,
            || None,
            None,
            Some(&unsafe_dir.0),
            None,
            &[],
            || None,
            euid,
        );
        assert_eq!(resolved, None);
    }

    #[test]
    fn falls_back_to_shell_lookup_only_when_no_socket_tier_has_loaded_keys() {
        let dir = TempDir::new("shell-fallback");
        let runtime_dir = dir.0.join("run");
        let loaded_runtime = runtime_dir.join("openssh_agent");
        let shell_sock = dir.0.join("from-shell.sock");
        let h1 = spawn_mock_agent(&loaded_runtime, identities_reply(1));
        let euid = unsafe { libc::geteuid() as u32 };

        let mut shell_called = false;
        let resolved = resolve_with(
            None,
            || None,
            Some(&runtime_dir),
            None,
            None,
            &[],
            || {
                shell_called = true;
                Some(shell_sock.clone())
            },
            euid,
        );
        h1.join().unwrap();
        assert!(!shell_called);
        assert_eq!(resolved.as_deref(), Some(loaded_runtime.as_path()));

        let empty_dir = TempDir::new("shell-fallback-empty");
        let empty_runtime_dir = empty_dir.0.join("run");
        let empty_runtime = empty_runtime_dir.join("ssh-agent.socket");
        let h_empty = spawn_mock_agent(&empty_runtime, identities_reply(0));
        let h_shell = spawn_mock_agent(&shell_sock, identities_reply(2));

        let resolved = resolve_with(
            None,
            || None,
            Some(&empty_runtime_dir),
            None,
            None,
            &[],
            || Some(shell_sock.clone()),
            euid,
        );
        h_empty.join().unwrap();
        h_shell.join().unwrap();
        assert_eq!(resolved.as_deref(), Some(shell_sock.as_path()));
    }

    #[test]
    fn parses_keychain_sh_and_fish_files() {
        assert_eq!(
            parse_keychain_ssh_auth_sock(
                b"SSH_AUTH_SOCK=/tmp/ssh-xyz/agent.123; export SSH_AUTH_SOCK;\nSSH_AGENT_PID=123; export SSH_AGENT_PID;\n"
            ),
            Some(PathBuf::from("/tmp/ssh-xyz/agent.123"))
        );
        assert_eq!(
            parse_keychain_ssh_auth_sock(
                b"set -e SSH_AUTH_SOCK; set -x -U SSH_AUTH_SOCK '/home/u/.ssh/agent/s.abc'\nset -e SSH_AGENT_PID; set -x -U SSH_AGENT_PID 456\n"
            ),
            Some(PathBuf::from("/home/u/.ssh/agent/s.abc"))
        );
        assert_eq!(
            parse_keychain_ssh_auth_sock(b"set -gx SSH_AUTH_SOCK /tmp/fish.sock\n"),
            Some(PathBuf::from("/tmp/fish.sock"))
        );
    }

    #[test]
    fn discovers_keychain_socket_for_current_hostname() {
        let dir = TempDir::new("keychain");
        let keychain_dir = dir.0.join(".keychain");
        std::fs::create_dir_all(&keychain_dir).unwrap();
        let keychain_sock = dir.0.join("active-agent.sock");
        let handle = spawn_mock_agent(&keychain_sock, identities_reply(1));
        std::fs::write(
            keychain_dir.join("testhost-fish"),
            format!("set -x -U SSH_AUTH_SOCK '{}'\n", keychain_sock.display()),
        )
        .unwrap();
        let euid = unsafe { libc::geteuid() as u32 };

        let mut shell_called = false;
        let resolved = resolve_with(
            None,
            || None,
            None,
            Some(&dir.0),
            Some("testhost.localdomain"),
            &[],
            || {
                shell_called = true;
                None
            },
            euid,
        );
        handle.join().unwrap();
        assert!(!shell_called);
        assert_eq!(resolved.as_deref(), Some(keychain_sock.as_path()));
    }

    #[test]
    fn parses_sentinel_framed_shell_env_output() {
        let raw = b"Welcome to shell\n__SEMACS_ENV_BEGIN__HOME=/home/u\0SSH_AUTH_SOCK=/home/u/.ssh/agent/s.123\0TERM=xterm\0__SEMACS_ENV_END__\nbye\n";
        assert_eq!(
            parse_shell_env_ssh_auth_sock(raw),
            Some(PathBuf::from("/home/u/.ssh/agent/s.123"))
        );
        assert_eq!(
            parse_shell_env_ssh_auth_sock(
                b"__SEMACS_ENV_BEGIN__SSH_AUTH_SOCK=\0__SEMACS_ENV_END__"
            ),
            None
        );
        assert_eq!(
            parse_shell_env_ssh_auth_sock(b"SSH_AUTH_SOCK=/tmp/no-sentinel"),
            None
        );
    }

    #[test]
    fn parses_systemd_show_environment_lines() {
        let raw = b"DISPLAY=:0\nSSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket\nWAYLAND_DISPLAY=wayland-1\n";
        assert_eq!(
            parse_systemd_ssh_auth_sock(raw),
            Some(PathBuf::from("/run/user/1000/ssh-agent.socket"))
        );
        assert_eq!(
            parse_systemd_ssh_auth_sock(b"SSH_AUTH_SOCK='/tmp/ssh-a/agent.1'\n"),
            Some(PathBuf::from("/tmp/ssh-a/agent.1"))
        );
        assert_eq!(parse_systemd_ssh_auth_sock(b"SSH_AUTH_SOCK=\n"), None);
        assert_eq!(parse_systemd_ssh_auth_sock(b"DISPLAY=:0\n"), None);
    }
}
