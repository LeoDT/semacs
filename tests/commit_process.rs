#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use semacs::commit::{Commit, EditorOutcome, EditorRequest, Event, Events, Repository};

#[path = "commit_process/exit.rs"]
mod exit;
#[path = "commit_process/exit_signals.rs"]
mod exit_signals;
#[path = "commit_process/history.rs"]
mod history;
#[path = "commit_process/merge_start.rs"]
mod merge_starts;
#[path = "commit_process/prompt.rs"]
mod prompts;
#[path = "commit_process/revisit.rs"]
mod revisits;
#[path = "commit_process/start.rs"]
mod starts;
#[path = "commit_process/environment.rs"]
mod starts_environment;

fn runtime() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".pi/tmp")
}

struct Fixture {
    base: PathBuf,
    repository: Repository,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = runtime().join(format!(
            "commit-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&base).unwrap();
        let root = base.join("repo");
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            base,
            repository: Repository {
                git_dir: root.join(".git"),
                root,
            },
        };
        fixture.git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        fixture.git(&["config", "user.name", "Fixture"]);
        fixture.git(&["config", "user.email", "fixture@example.invalid"]);
        fixture.write("a", "staged\n");
        fixture.git(&["add", "a"]);
        fixture
    }

    fn git(&self, arguments: &[&str]) -> Vec<u8> {
        let result = Command::new("git")
            .arg("-C")
            .arg(&self.repository.root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    }

    fn write(&self, path: &str, content: &str) {
        fs::write(self.repository.root.join(path), content).unwrap();
    }

    fn hook(&self, name: &str, script: &str) {
        let path = self.repository.git_dir.join("hooks").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn start(&self) -> (Commit, Events) {
        self.start_using(Path::new(env!("CARGO_BIN_EXE_semacs")))
    }

    fn start_using(&self, executable: &Path) -> (Commit, Events) {
        Commit::start(&self.repository, executable, &runtime()).unwrap()
    }

    fn head(&self) -> Option<Vec<u8>> {
        let result = Command::new("git")
            .arg("-C")
            .arg(&self.repository.root)
            .args(["rev-parse", "--verify", "HEAD"])
            .output()
            .unwrap();
        result.status.success().then_some(result.stdout)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).unwrap();
    }
}

fn next(events: &Events, output: &mut Vec<u8>) -> Event {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("commit did not reach its next event");
        match event {
            Event::Output(bytes) => output.extend(bytes),
            Event::Diagnostic(error) | Event::WaitFailed(error) => {
                panic!("commit diagnostic: {error}")
            }
            event => return event,
        }
    }
}

fn edit(events: &Events, output: &mut Vec<u8>) -> EditorRequest {
    match next(events, output) {
        Event::Edit(request) => request,
        event => panic!(
            "expected editor request, got {event:?}: {}",
            String::from_utf8_lossy(output)
        ),
    }
}

fn finish(commit: Commit, events: &Events, output: &mut Vec<u8>) -> ExitStatus {
    let status = match next(events, output) {
        Event::Exited(status) => status,
        event => panic!("expected process exit, got {event:?}"),
    };
    commit.join().unwrap();
    status
}

fn wait_output(events: &Events, output: &mut Vec<u8>, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !String::from_utf8_lossy(output).contains(marker) {
        assert!(
            Instant::now() < deadline,
            "output marker not seen: {marker}"
        );
        match events.recv_timeout(Duration::from_secs(1)).unwrap() {
            Event::Output(bytes) => output.extend(bytes),
            event => panic!("expected output marker {marker}, got {event:?}"),
        }
    }
}

fn normal_hooks_and_git_cleanup() {
    let fixture = Fixture::new();
    fixture.write(
        "template.txt",
        "template subject\n\n# template instruction\n",
    );
    fixture.git(&["config", "commit.template", "template.txt"]);
    fixture.hook(
        "pre-commit",
        "echo pre >> .git/order\nprintf 'from-hook\\n' > a\ngit add a",
    );
    fixture.hook(
        "prepare-commit-msg",
        "echo prepare >> .git/order\nprintf '%s\\n' \"${2:-}\" > .git/message-source\nprintf 'prepared\\n\\n# instruction\\n' > \"$1\"",
    );
    fixture.hook(
        "commit-msg",
        "echo commit-msg >> .git/order\ncat \"$1\" > .git/hook-message",
    );
    fixture.hook("post-commit", "echo post >> .git/order");
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    assert_eq!(
        request.path,
        fixture.repository.git_dir.join("COMMIT_EDITMSG")
    );
    assert_eq!(
        fs::read_to_string(&request.path).unwrap(),
        "prepared\n\n# instruction\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repository.git_dir.join("order")).unwrap(),
        "pre\nprepare\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repository.git_dir.join("message-source")).unwrap(),
        "template\n"
    );
    fs::write(&request.path, "actual title\n\nbody\n# instruction\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(
        finish(commit, &events, &mut output).success(),
        "{}",
        String::from_utf8_lossy(&output)
    );
    assert_eq!(fixture.git(&["show", "HEAD:a"]), b"from-hook\n");
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%B"]),
        b"actual title\n\nbody\n\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.repository.git_dir.join("order")).unwrap(),
        "pre\nprepare\ncommit-msg\npost\n"
    );
    assert!(
        fs::read_to_string(fixture.repository.git_dir.join("hook-message"))
            .unwrap()
            .contains("# instruction")
    );
}

fn cancel_and_failures_preserve_messages_and_side_effects() {
    for failure in [
        "cancel",
        "drop-request",
        "pre-commit",
        "commit-msg",
        "empty",
    ] {
        let fixture = Fixture::new();
        fixture.hook(
            "pre-commit",
            if failure == "pre-commit" {
                "echo PRE_DENIED >&2\nexit 1"
            } else {
                "echo side-effect > .git/effect"
            },
        );
        fixture.hook(
            "commit-msg",
            if failure == "commit-msg" {
                "echo MESSAGE_DENIED >&2\nexit 1"
            } else {
                "echo ran > .git/commit-msg-ran"
            },
        );
        let (commit, events) = fixture.start();
        let mut output = Vec::new();
        let message;
        if failure != "pre-commit" {
            let request = edit(&events, &mut output);
            message = Some(request.path.clone());
            fs::write(
                &request.path,
                if failure == "empty" {
                    "# only comments\n"
                } else {
                    "recover this message\n"
                },
            )
            .unwrap();
            if failure == "drop-request" {
                drop(request);
            } else {
                request
                    .respond(if failure == "cancel" {
                        EditorOutcome::Cancel
                    } else {
                        EditorOutcome::Finish
                    })
                    .unwrap();
            }
        } else {
            message = None;
        }
        assert!(!finish(commit, &events, &mut output).success(), "{failure}");
        assert!(fixture.head().is_none());
        if let Some(message) = message {
            assert!(
                fs::read_to_string(message)
                    .unwrap()
                    .contains(if failure == "empty" {
                        "only comments"
                    } else {
                        "recover this message"
                    })
            );
            assert!(fixture.repository.git_dir.join("effect").exists());
        }
        if failure == "cancel" {
            assert!(!fixture.repository.git_dir.join("commit-msg-ran").exists());
        }
        if failure == "pre-commit" {
            assert!(String::from_utf8_lossy(&output).contains("PRE_DENIED"));
        }
        if failure == "commit-msg" {
            assert!(String::from_utf8_lossy(&output).contains("MESSAGE_DENIED"));
        }
    }
}

fn message_ring_recovers_cancel_and_hook_failure_without_cleaning_git_files() {
    use semacs::buffer::{Buffer, Edit, EditSource};
    use semacs::commit::message::{MessagePosition, MessageRing};

    let fixture = Fixture::new();
    fixture.write("template.txt", "\n; template instructions\n");
    fixture.git(&["config", "commit.template", "template.txt"]);
    fixture.git(&["config", "core.commentChar", ";"]);
    fixture.git(&["config", "commit.cleanup", "verbatim"]);
    fixture.hook(
        "commit-msg",
        "cp \"$1\" .git/hook-message\nif test -f .git/deny; then exit 1; fi",
    );
    let mut ring = MessageRing::default();
    let draft = "recover this subject\n\nbody  \n; old instructions\n; - >8 -\nold patch\n";
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    assert!(ring.save(draft, ";"));
    fs::write(&request.path, draft).unwrap();
    let original_path = request.path.clone();
    request.respond(EditorOutcome::Cancel).unwrap();
    assert!(!finish(commit, &events, &mut output).success());
    assert_eq!(fs::read_to_string(original_path).unwrap(), draft);
    assert!(fixture.head().is_none());
    assert!(!fixture.repository.git_dir.join("hook-message").exists());

    for deny in [true, false] {
        let marker = fixture.repository.git_dir.join("deny");
        if deny {
            fs::write(&marker, "deny").unwrap();
        } else {
            fs::remove_file(&marker).unwrap();
        }
        let (commit, events) = fixture.start();
        let request = edit(&events, &mut output);
        let loaded = semacs::files::load(&request.path).unwrap();
        let mut buffer = Buffer::visiting(request.path.clone(), loaded.file, loaded.stamp);
        let initial = buffer.text().to_string();
        assert!(!ring.save(&initial, ";"));
        let recalled = ring
            .recall(&mut MessagePosition::default(), &initial, ";", 1)
            .unwrap();
        assert_eq!(recalled.text, "recover this subject\n\nbody  \n");
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: recalled.range,
                text: &recalled.text,
                source: EditSource::Command,
            })
            .unwrap();
        let text = buffer.text().to_string();
        assert!(text.contains("; template instructions"));
        assert!(!text.contains("old instructions"));
        assert!(!text.contains("old patch"));
        assert!(ring.save(&text, ";"));
        semacs::files::write_atomic(&request.path, &text, buffer.format()).unwrap();
        request.respond(EditorOutcome::Finish).unwrap();
        assert_eq!(finish(commit, &events, &mut output).success(), !deny);
        assert_eq!(
            fs::read_to_string(fixture.repository.git_dir.join("hook-message")).unwrap(),
            text
        );
        if deny {
            assert!(fixture.head().is_none());
        } else {
            let committed = String::from_utf8(fixture.git(&["log", "-1", "--format=%B"])).unwrap();
            assert!(committed.starts_with("recover this subject\n\nbody  \n"));
            assert!(committed.contains("; template instructions"));
        }
    }
}

fn detached_and_linked_worktree_commits() {
    let mut fixture = Fixture::new();
    fixture.git(&["commit", "-qm", "initial"]);
    let initial = fixture.head().unwrap();
    fixture.git(&["checkout", "--detach", "--quiet"]);
    fixture.write("a", "detached change\n");
    fixture.git(&["add", "a"]);
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    fs::write(&request.path, "detached commit\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(finish(commit, &events, &mut output).success());
    assert_eq!(fixture.git(&["rev-parse", "refs/heads/main"]), initial);
    assert_eq!(fixture.git(&["show", "HEAD:a"]), b"detached change\n");
    let detached = fixture.head().unwrap();
    let linked = fixture.base.join("linked");
    fixture.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "linked",
        linked.to_str().unwrap(),
    ]);
    fixture.repository.root = linked;
    let directory = fixture.git(&["rev-parse", "--absolute-git-dir"]);
    fixture.repository.git_dir = PathBuf::from(OsString::from_vec(
        directory.strip_suffix(b"\n").unwrap().to_vec(),
    ));
    fixture.write("a", "linked change\n");
    fixture.git(&["add", "a"]);
    let (commit, events) = fixture.start();
    let request = edit(&events, &mut output);
    assert_eq!(
        request.path,
        fixture.repository.git_dir.join("COMMIT_EDITMSG")
    );
    fs::write(&request.path, "linked commit\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(finish(commit, &events, &mut output).success());
    assert_eq!(fixture.git(&["show", "HEAD:a"]), b"linked change\n");
    assert_eq!(
        Command::new("git")
            .arg("-C")
            .arg(fixture.base.join("repo"))
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
        detached
    );
}

fn signing_is_not_disabled() {
    let fixture = Fixture::new();
    let signer = fixture.base.join("fake-signing-program");
    fs::write(
        &signer,
        "#!/bin/sh\ncat > .git/signing-input\necho SIGNING_REACHED >&2\nexit 42\n",
    )
    .unwrap();
    fs::set_permissions(&signer, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.git(&["config", "gpg.program", signer.to_str().unwrap()]);
    fixture.git(&["config", "commit.gpgsign", "true"]);
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    fs::write(&request.path, "signed message\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(!finish(commit, &events, &mut output).success());
    assert!(String::from_utf8_lossy(&output).contains("SIGNING_REACHED"));
    assert!(
        fs::read_to_string(fixture.repository.git_dir.join("signing-input"))
            .unwrap()
            .contains("signed message")
    );
    assert!(fixture.head().is_none());
}

fn controlling_terminal_accepts_input_without_echoing_it() {
    let fixture = Fixture::new();
    fixture.hook("pre-commit", "test -t 0 && test -t 1 && test -t 2\nprintf HOOK_INPUT: > /dev/tty\nIFS= read -r answer < /dev/tty\nprintf '%s' \"$answer\" > .git/input");
    let (mut commit, events) = fixture.start();
    let mut output = Vec::new();
    wait_output(&events, &mut output, "HOOK_INPUT:");
    let input = b"private-test-answer\n";
    let mut written = 0;
    while written < input.len() {
        match commit.try_write_input(&input[written..]) {
            Ok(count) => {
                assert!(count > 0);
                written += count;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::yield_now()
            }
            Err(error) => panic!("cannot send PTY input: {error}"),
        }
    }
    let request = edit(&events, &mut output);
    assert_eq!(
        fs::read(fixture.repository.git_dir.join("input")).unwrap(),
        b"private-test-answer"
    );
    fs::write(&request.path, "tty commit\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(finish(commit, &events, &mut output).success());
    assert!(!String::from_utf8_lossy(&output).contains("private-test-answer"));
}

fn interrupt_then_force_kill() {
    for ignore in [false, true] {
        let fixture = Fixture::new();
        fixture.hook(
            "pre-commit",
            if ignore {
                "trap '' INT\nkill -STOP \"$PPID\"\necho BLOCKED\nwhile :; do sleep 1; done"
            } else {
                "trap 'exit 130' INT\necho BLOCKED\nwhile :; do sleep 1; done"
            },
        );
        let (commit, events) = fixture.start();
        let mut output = Vec::new();
        wait_output(&events, &mut output, "BLOCKED");
        commit.interrupt().unwrap();
        if ignore {
            std::thread::sleep(Duration::from_millis(100));
            commit.kill().unwrap();
        }
        let status = finish(commit, &events, &mut output);
        assert!(!status.success());
        if ignore {
            assert_eq!(status.signal(), Some(libc::SIGKILL));
        }
        assert!(fixture.head().is_none());
    }
}

fn output_backpressure_and_receiver_drop_reclaim_processes() {
    let fixture = Fixture::new();
    fixture.hook(
        "pre-commit",
        "head -c 262144 /dev/zero | tr '\\000' x\nprintf '\\nOUTPUT_END\\n'",
    );
    let (commit, events) = fixture.start();
    std::thread::sleep(Duration::from_millis(100));
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    assert!(output.iter().filter(|byte| **byte == b'x').count() >= 262144);
    assert!(String::from_utf8_lossy(&output).contains("OUTPUT_END"));
    request.respond(EditorOutcome::Cancel).unwrap();
    assert!(!finish(commit, &events, &mut output).success());
    let (commit, events) = fixture.start();
    std::thread::sleep(Duration::from_millis(100));
    drop(events);
    commit.join().unwrap();
    assert!(fixture.head().is_none());
    fixture.hook("pre-commit", "echo QUIET_WAIT\nwhile :; do sleep 1; done");
    let (commit, events) = fixture.start();
    wait_output(&events, &mut output, "QUIET_WAIT");
    drop(events);
    commit.join().unwrap();
}

fn drop_owner_cancels_live_editor_and_cleans_bridge() {
    let fixture = Fixture::new();
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    drop(commit);
    loop {
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Output(bytes)) => output.extend(bytes),
            Err(RecvTimeoutError::Disconnected) => break,
            event => panic!("unexpected abandoned-session event: {event:?}"),
        }
    }
    assert!(request.respond(EditorOutcome::Finish).is_err());
    assert!(fixture.head().is_none());
    assert!(!fs::read_dir(runtime()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(&format!(".semacs-editor-{}-", std::process::id()))
    }));
}

fn helper_paths_preserve_quotes_newlines_and_non_utf8() {
    let mut fixture = Fixture::new();
    let root = fixture
        .base
        .join(OsString::from_vec(b"repo ' \n\xff".to_vec()));
    fs::rename(&fixture.repository.root, &root).unwrap();
    fixture.repository = Repository {
        git_dir: root.join(".git"),
        root,
    };
    let helper = fixture
        .base
        .join(OsString::from_vec(b"editor ' \n\xff".to_vec()));
    symlink(env!("CARGO_BIN_EXE_semacs"), &helper).unwrap();
    let (commit, events) = fixture.start_using(&helper);
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    assert_eq!(
        request.path,
        fixture.repository.git_dir.join("COMMIT_EDITMSG")
    );
    fs::write(&request.path, "path bytes intact\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(
        finish(commit, &events, &mut output).success(),
        "{}",
        String::from_utf8_lossy(&output)
    );
}

fn hook_editor_requests_and_external_head_changes_follow_git() {
    let fixture = Fixture::new();
    fixture.hook(
        "pre-commit",
        "eval \"$GIT_EDITOR hook-message\"\ntest -f hook-message",
    );
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    assert_eq!(request.path, fixture.repository.root.join("hook-message"));
    fs::write(&request.path, "hook editor\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    let request = edit(&events, &mut output);
    let tree = fixture.git(&["write-tree"]);
    let tree = std::str::from_utf8(&tree).unwrap().trim_end();
    let oid = fixture.git(&["commit-tree", tree, "-m", "external commit"]);
    let oid = std::str::from_utf8(&oid).unwrap().trim_end();
    fixture.git(&["update-ref", "HEAD", oid]);
    fs::write(&request.path, "after external HEAD change\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    let status = finish(commit, &events, &mut output);
    assert!(!status.success());
    assert!(
        String::from_utf8_lossy(&output).contains("cannot lock ref"),
        "{}",
        String::from_utf8_lossy(&output)
    );
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%s"]),
        b"external commit\n"
    );
}

fn live_editor_is_not_subject_to_the_f2_fifteen_second_timeout() {
    let fixture = Fixture::new();
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    let request = edit(&events, &mut output);
    std::thread::sleep(Duration::from_secs(16));
    fs::write(&request.path, "long editing session\n").unwrap();
    request.respond(EditorOutcome::Finish).unwrap();
    assert!(finish(commit, &events, &mut output).success());
}

fn pre_start_and_editor_start_failures_are_distinct() {
    let fixture = Fixture::new();
    let missing = Repository {
        root: fixture.base.join("missing"),
        git_dir: fixture.base.join("missing/.git"),
    };
    let error = match Commit::start(
        &missing,
        Path::new(env!("CARGO_BIN_EXE_semacs")),
        &runtime(),
    ) {
        Ok(_) => panic!("missing worktree unexpectedly started"),
        Err(error) => error,
    };
    assert!(!error.started);
    let (commit, events) = fixture.start_using(&fixture.base.join("nonexistent-editor"));
    let mut output = Vec::new();
    assert!(!finish(commit, &events, &mut output).success());
    assert!(fixture.head().is_none());
    assert!(String::from_utf8_lossy(&output).contains("editor"));
    fixture.git(&["commit", "-qm", "baseline"]);
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    assert!(!finish(commit, &events, &mut output).success());
    assert!(String::from_utf8_lossy(&output).contains("nothing to commit"));
}

#[test]
fn isolated_commit_process_child() {
    if std::env::var_os("SEMACS_COMMIT_TEST_CHILD").is_none() {
        return;
    }
    for (name, test) in [
        (
            "normal hooks and cleanup",
            normal_hooks_and_git_cleanup as fn(),
        ),
        (
            "cancel and failures",
            cancel_and_failures_preserve_messages_and_side_effects,
        ),
        (
            "message ring after cancel and failure",
            message_ring_recovers_cancel_and_hook_failure_without_cleaning_git_files,
        ),
        ("detached and linked", detached_and_linked_worktree_commits),
        ("signing", signing_is_not_disabled),
        (
            "PTY input",
            controlling_terminal_accepts_input_without_echoing_it,
        ),
        ("signals", interrupt_then_force_kill),
        (
            "output and disconnect",
            output_backpressure_and_receiver_drop_reclaim_processes,
        ),
        (
            "owner drop",
            drop_owner_cancels_live_editor_and_cleans_bridge,
        ),
        (
            "path bytes",
            helper_paths_preserve_quotes_newlines_and_non_utf8,
        ),
        (
            "hook editor and HEAD",
            hook_editor_requests_and_external_head_changes_follow_git,
        ),
        (
            "long editor",
            live_editor_is_not_subject_to_the_f2_fifteen_second_timeout,
        ),
        (
            "startup failures",
            pre_start_and_editor_start_failures_are_distinct,
        ),
    ] {
        eprintln!("commit case: {name}");
        test();
    }
}

#[test]
fn actual_git_editor_and_process_matrix_runs_without_user_configuration_or_desktop() {
    isolated_test("isolated_commit_process_child", "SEMACS_COMMIT_TEST_CHILD");
}

#[gpui_kit::test]
fn isolated_commit_editor_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_UI_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::AppShell;
    use semacs::buffer::Buffer;
    use semacs::mode::ModeId;
    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    for phase in ["cancel", "reject", "finish"] {
        let fixture = Fixture::new();
        fixture.hook(
            "commit-msg",
            if phase == "reject" {
                "cp \"$1\" .git/received-message\nexit 1"
            } else {
                "cp \"$1\" .git/received-message"
            },
        );
        let (commit, events) = fixture.start();
        let mut output = Vec::new();
        let request = edit(&events, &mut output);
        let path = request.path.clone();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.edit_commit_message(request, "#".into(), window, cx);
            })
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "editor UI did not open");
        }
        if phase == "cancel" {
            visual.simulate_input("recovered from UI\n\n");
        } else {
            visual.simulate_keystrokes("alt-p");
        }
        let text = visual.read(|cx| {
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
        });
        assert!(text.starts_with("recovered from UI\n"));
        assert!(text.contains("#"));
        visual.simulate_keystrokes(if phase == "cancel" {
            "ctrl-c ctrl-k"
        } else {
            "ctrl-c ctrl-c"
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual.read(|cx| {
                shell
                    .read(cx)
                    .buffers()
                    .all(|buffer| buffer.mode() != ModeId::GIT_COMMIT)
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "editor UI did not finish");
        }
        let status = finish(commit, &events, &mut output);
        assert_eq!(status.success(), phase == "finish", "{phase}");
        assert_eq!(fs::read_to_string(path).unwrap(), text);
        if phase == "cancel" {
            assert!(!fixture.repository.git_dir.join("received-message").exists());
        } else {
            assert_eq!(
                fs::read_to_string(fixture.repository.git_dir.join("received-message")).unwrap(),
                text
            );
        }
        if phase == "finish" {
            assert_eq!(
                fixture.git(&["log", "-1", "--format=%B"]),
                b"recovered from UI\n\n"
            );
        } else {
            assert!(fixture.head().is_none());
        }
    }
}

#[test]
fn actual_git_helper_uses_headless_editor_save_cancel_and_recovery() {
    isolated_test("isolated_commit_editor_ui_child", "SEMACS_COMMIT_UI_CHILD");
}

#[gpui_kit::test]
fn isolated_commit_session_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_SESSION_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    for phase in [
        "finish",
        "cancel",
        "reject",
        "before-editor",
        "background",
        "terminated",
    ] {
        eprintln!("commit session phase: {phase}");
        visual.update(|_, cx| {
            shell.update(cx, |shell, _| {
                shell.set_commit_runtime(semacs::commit::Runtime {
                    executable: PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    directory: runtime(),
                });
            })
        });
        let fixture = Fixture::new();
        fixture.git(&["config", "core.commentChar", ";"]);
        fixture.hook(
            "pre-commit",
            if phase == "before-editor" {
                "echo PRE_REJECT; exit 1"
            } else if phase == "background" {
                "echo WAITING; while test ! -e .git/release; do sleep 0.01; done"
            } else if phase == "finish" {
                "head -c 262144 /dev/zero | tr '\\000' X; echo PRE_OUTPUT"
            } else {
                "echo PRE_OUTPUT"
            },
        );
        fixture.hook("prepare-commit-msg", "echo \"$PPID\" > .git/git-pid");
        fixture.hook(
            "commit-msg",
            if phase == "reject" {
                "echo HOOK_REJECT; exit 1"
            } else {
                "echo HOOK_ACCEPT"
            },
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let index = shell.create_buffer(
                    Buffer::new(
                        Some(fixture.repository.root.join("a")),
                        DecodedFile {
                            text: "unsaved\n".into(),
                            format: FileFormat::default(),
                        },
                    ),
                    window,
                    cx,
                );
                shell.activate(index, window, cx);
            })
        });
        visual.simulate_keystrokes("ctrl-x 1 alt-x");
        visual.simulate_input("magit-status");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_STATUS
            })
        });
        let count = visual.read(|cx| shell.read(cx).commit_records().count());
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input("magit-commit-create");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| shell.read(cx).commit_records().count() > count)
        });
        let operation = visual.read(|cx| shell.read(cx).commit_records().last().unwrap().operation);
        if phase == "before-editor" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    !shell
                        .read(cx)
                        .commit_records()
                        .find(|record| record.operation == operation)
                        .unwrap()
                        .phase
                        .is_active()
                })
            });
            visual.read(|cx| {
                let record = shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap();
                assert!(matches!(record.phase, CommitPhase::Exited(status) if !status.success()));
                assert!(String::from_utf8_lossy(&record.output).contains("PRE_REJECT"));
                assert!(record.diagnostics.is_empty());
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::MAGIT_STATUS
                );
            });
            continue;
        }
        if phase == "background" {
            visual.simulate_keystrokes("alt-x");
            fs::write(fixture.repository.git_dir.join("release"), "").unwrap();
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell
                        .read(cx)
                        .buffers()
                        .any(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                })
            });
            visual.read(|cx| {
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::MAGIT_STATUS
                );
            });
            visual.simulate_keystrokes("ctrl-g alt-x");
            visual.simulate_input("magit-diff-staged");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_DIFF
                })
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let index = shell
                        .buffers()
                        .position(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                        .unwrap();
                    shell.activate(index, window, cx);
                })
            });
        } else {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
                })
            });
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).buffers().any(|buffer| {
                        buffer.mode() == ModeId::MAGIT_DIFF
                            && buffer.text().to_string().contains("+staged")
                    })
                })
            });
            for _ in 0..4 {
                visual.simulate_keystrokes("ctrl-x o");
                if visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_DIFF
                }) {
                    break;
                }
            }
            visual.read(|cx| {
                let view = shell.read(cx).active_view().read(cx);
                assert_eq!(view.buffer().mode(), ModeId::MAGIT_DIFF);
                assert!(view.buffer().text().to_string().contains("+staged"));
            });
            for _ in 0..4 {
                visual.simulate_keystrokes("ctrl-x o");
                if visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
                }) {
                    break;
                }
            }
            visual.read(|cx| {
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::GIT_COMMIT
                )
            });
        }
        if phase == "finish" {
            visual.simulate_keystrokes("alt-x");
            visual.simulate_input("reload-configuration");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).message()
                        == Some("Configuration reloaded")
                })
            });
        }
        let message_path = fixture.repository.git_dir.join("COMMIT_EDITMSG");
        if phase == "terminated" {
            let pid: i32 = fs::read_to_string(fixture.repository.git_dir.join("git-pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        } else {
            visual.simulate_input("session message\n\n");
            visual.simulate_keystrokes(if phase == "cancel" {
                "ctrl-c ctrl-k"
            } else {
                "ctrl-c ctrl-c"
            });
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                !shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap()
                    .phase
                    .is_active()
            })
        });
        visual.read(|cx| {
            let record = shell
                .read(cx)
                .commit_records()
                .find(|record| record.operation == operation)
                .unwrap();
            let success = matches!(record.phase, CommitPhase::Exited(status) if status.success());
            assert_eq!(
                success,
                phase == "finish" || phase == "background",
                "{phase}"
            );
            assert!(record.diagnostics.is_empty(), "{:?}", record.diagnostics);
            if phase == "finish" {
                assert!(record.output.starts_with(&vec![b'X'; 262144]));
            }
            assert!(
                shell
                    .read(cx)
                    .buffers()
                    .all(|buffer| buffer.mode() != ModeId::GIT_COMMIT)
            );
            if phase != "background" && phase != "terminated" {
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::MAGIT_STATUS
                );
            }
        });
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).buffers().any(|buffer| {
                    buffer
                        .status_data()
                        .is_some_and(|data| data.repository() == &fixture.repository)
                        && !buffer.git_is_stale()
                })
            })
        });
        visual.read(|cx| {
            let shell = shell.read(cx);
            let data = shell
                .buffers()
                .find_map(|buffer| {
                    buffer
                        .status_data()
                        .filter(|data| data.repository() == &fixture.repository)
                })
                .unwrap();
            let success = phase == "finish" || phase == "background";
            assert_eq!(
                data.group(semacs::status::Group::Staged).count(),
                usize::from(!success)
            );
        });
        if phase == "terminated" {
            visual.simulate_input("still editable");
            visual.read(|cx| {
                assert!(
                    shell
                        .read(cx)
                        .active_view()
                        .read(cx)
                        .buffer()
                        .text()
                        .to_string()
                        .contains("still editable")
                )
            });
        } else {
            assert!(
                fs::read_to_string(message_path)
                    .unwrap()
                    .starts_with("session message\n")
            );
        }
        if phase == "finish" || phase == "background" {
            assert_eq!(
                fixture.git(&["log", "-1", "--format=%B"]),
                b"session message\n\n"
            );
        } else {
            assert!(fixture.head().is_none());
        }
    }

    let fixture = Fixture::new();
    let operation = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.start_commit(
                fixture.repository.clone(),
                PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                fixture.base.join("missing-runtime"),
                window,
                cx,
            )
        })
    });
    pump(&mut visual, |visual| {
        visual.read(|cx| {
            !shell
                .read(cx)
                .commit_records()
                .find(|record| record.operation == operation)
                .unwrap()
                .phase
                .is_active()
        })
    });
    visual.read(|cx| {
        assert!(matches!(
            shell
                .read(cx)
                .commit_records()
                .find(|record| record.operation == operation)
                .unwrap()
                .phase,
            CommitPhase::Failed { started: false, .. }
        ))
    });
}

#[gpui_kit::test]
fn isolated_commit_process_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_PROCESS_UI_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    for ignore in [false, true] {
        let fixture = Fixture::new();
        fixture.hook(
            "pre-commit",
            if ignore {
                "trap '' INT\nkill -STOP \"$PPID\"\necho BLOCKED\nwhile :; do sleep 1; done"
            } else {
                "trap 'exit 130' INT\necho BLOCKED\nwhile :; do sleep 1; done"
            },
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let index = shell.create_buffer(
                    Buffer::new(
                        Some(fixture.repository.root.join("a")),
                        DecodedFile {
                            text: "unsaved".into(),
                            format: FileFormat::default(),
                        },
                    ),
                    window,
                    cx,
                );
                shell.activate(index, window, cx);
            })
        });
        visual.simulate_keystrokes("ctrl-x 1 alt-x");
        visual.simulate_input("magit-process-buffer");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_PROCESS
            })
        });
        visual.simulate_keystrokes("q alt-x");
        visual.simulate_input("magit-status");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_STATUS
            })
        });
        let operation = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.start_commit(
                    fixture.repository.clone(),
                    PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    runtime(),
                    window,
                    cx,
                )
            })
        });
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                let record = shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap();
                String::from_utf8_lossy(&record.output).contains("BLOCKED")
            })
        });
        visual.simulate_keystrokes("$");
        let process_buffer = visual.read(|cx| {
            let buffer = shell.read(cx).active_view().read(cx).buffer();
            assert_eq!(buffer.mode(), ModeId::MAGIT_PROCESS);
            assert!(buffer.text().to_string().contains("BLOCKED"));
            buffer.snapshot_id().buffer
        });
        visual.simulate_keystrokes("k");
        visual.read(|cx| {
            assert_eq!(
                shell.read(cx).session().unwrap().kind(),
                &ReadKind::CommitInterrupt { operation }
            )
        });
        visual.simulate_keystrokes("n q alt-x");
        visual.simulate_input("reload-configuration");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).message() == Some("Configuration reloaded")
            })
        });
        visual.simulate_keystrokes("$");
        visual.read(|cx| {
            assert_eq!(
                shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .snapshot_id()
                    .buffer,
                process_buffer
            )
        });
        visual.simulate_keystrokes("ctrl-x k enter");
        visual.read(|cx| {
            assert!(
                shell
                    .read(cx)
                    .buffers()
                    .all(|buffer| buffer.snapshot_id().buffer != process_buffer)
            );
            assert!(
                shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap()
                    .phase
                    .is_active()
            );
        });
        visual.simulate_keystrokes("$ k y");
        if ignore {
            visual.read(|cx| {
                assert!(
                    shell
                        .read(cx)
                        .commit_records()
                        .find(|record| record.operation == operation)
                        .unwrap()
                        .phase
                        .is_active()
                )
            });
            visual.simulate_keystrokes("k");
            visual.read(|cx| assert!(shell.read(cx).session().is_none()));
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                !shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap()
                    .phase
                    .is_active()
            })
        });
        visual.read(|cx| {
            let record = shell
                .read(cx)
                .commit_records()
                .find(|record| record.operation == operation)
                .unwrap();
            match record.phase {
                CommitPhase::Exited(status) => {
                    assert!(!status.success());
                    if ignore {
                        assert_eq!(status.signal(), Some(libc::SIGKILL));
                    }
                }
                ref phase => panic!("expected actual Git terminal status, got {phase:?}"),
            }
            let buffer = shell.read(cx).active_view().read(cx).buffer();
            assert_eq!(buffer.mode(), ModeId::MAGIT_PROCESS);
            assert!(!buffer.text().to_string().contains("running  git commit"));
        });
        visual.simulate_keystrokes("k");
        visual.read(|cx| {
            assert!(shell.read(cx).session().is_none());
            assert_eq!(
                shell.read(cx).active_view().read(cx).message(),
                Some("Process no longer running")
            );
        });
        assert!(fixture.head().is_none());
        visual.simulate_keystrokes("q");
    }
}

#[test]
fn actual_process_buffer_retains_history_and_interrupts_then_kills_its_owned_git() {
    isolated_test(
        "isolated_commit_process_ui_child",
        "SEMACS_COMMIT_PROCESS_UI_CHILD",
    );
}

#[track_caller]
fn pump(
    visual: &mut gpui_kit::VisualTestContext,
    ready: impl Fn(&gpui_kit::VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if !visual.windows().is_empty() {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        if ready(visual) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "commit session did not reach its barrier"
        );
        std::thread::yield_now();
    }
}

#[test]
fn actual_commit_session_owns_helper_editor_diff_output_and_terminal() {
    isolated_test(
        "isolated_commit_session_ui_child",
        "SEMACS_COMMIT_SESSION_CHILD",
    );
}

fn isolated_test(name: &str, flag: &str) {
    isolated_test_using(name, flag, |_| {});
}

fn isolated_test_using(name: &str, flag: &str, configure: impl FnOnce(&mut Command)) {
    fs::create_dir_all(runtime()).unwrap();
    let config = runtime().join(format!("commit-config-{}-{flag}", std::process::id()));
    fs::create_dir(&config).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", &config)
        .env(flag, "1");
    configure(&mut command);
    let output = command.output().unwrap();
    fs::remove_dir(config).unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
