use super::tests::{fixture, request, staged};
use super::*;
use std::os::unix::fs::PermissionsExt;

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[test]
fn signalled_write_child() {
    if std::env::var_os("SEMACS_INDEX_SIGNAL_CHILD").is_none() {
        return;
    }
    let repo = fixture();
    repo.write("a", "written before lost exit\n");
    let report = request(&repo, "a", Group::Unstaged, Operation::Stage).execute();
    assert_eq!(report.outcome, Outcome::Uncertain, "{}", report.diagnostic);
    assert!(report.stale);
    let status = report.verification.unwrap().unwrap();
    assert!(
        status
            .entries()
            .iter()
            .any(|entry| entry.path == Path::new("a") && entry.in_group(Group::Staged))
    );
    assert_eq!(staged(&repo, "a"), b"written before lost exit\n");
}

#[test]
fn signalled_write_is_not_replayed_or_inferred_successful_from_final_state() {
    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let fixture = fixture();
    let shim = fixture.0.join("git");
    let count = fixture.0.join("write-count");
    let source = format!(
        "#!/bin/sh\nfor arg do\n case \"$arg\" in\n core.hooksPath=*semacs-empty-hooks-*)\n printf x >> {}\n {} \"$@\"\n kill -KILL $$\n exit 99;;\n esac\ndone\nexec {} \"$@\"\n",
        quote(&count),
        quote(&real_git),
        quote(&real_git)
    );
    std::fs::write(&shim, source).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(fixture.0.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "status::index::process_tests::signalled_write_child",
            "--nocapture",
        ])
        .env("SEMACS_INDEX_SIGNAL_CHILD", "1")
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&count).unwrap(), b"x");
}
