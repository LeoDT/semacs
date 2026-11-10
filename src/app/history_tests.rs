use super::*;
use crate::app::git_test_support::{command, open, until, wait};
use crate::diff::tests::Repository as Fixture;
use crate::mode::ModeId;
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext};

fn point(visual: &mut VisualTestContext, shell: &Entity<AppShell>, text: &str) {
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        view.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find(text).unwrap();
            view.show_location(ByteOffset(offset), window, cx);
        });
    });
}

fn fixture() -> Fixture {
    let repo = Fixture::new();
    repo.write("a", "base\n");
    repo.commit();
    repo.write("a", "changed\n");
    repo.commit();
    repo
}

#[gpui::test]
fn history_navigation_loads_real_commit_and_blob_then_quits_locally(cx: &mut TestAppContext) {
    let repo = fixture();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    let log = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        assert_eq!(log.read(cx).buffer().mode(), ModeId::MAGIT_LOG);
        assert!(log.read(cx).buffer().history_commit().is_some());
    });
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    let revision = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        assert_eq!(revision.read(cx).buffer().mode(), ModeId::MAGIT_REVISION);
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| assert_eq!(shell.read(cx).active_view(), &log));
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    point(&mut visual, &shell, "-base");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert!(buffer.is_read_only());
        assert_eq!(buffer.text().to_string(), "base\n");
    });
}

#[gpui::test]
fn history_limit_refresh_preserves_independent_view_points_and_failure_state(
    cx: &mut TestAppContext,
) {
    let repo = fixture();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            let mut query = origin.read(cx).buffer().history_query().unwrap();
            if let Query::Log { limit, .. } = &mut query {
                *limit = 1;
            }
            shell.refresh_history_query(&origin, query, window, cx);
        })
    });
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::SplitWindowRight);
    let before = visual.read(|cx| {
        shell
            .read(cx)
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().mode() == ModeId::MAGIT_LOG)
            .map(|view| {
                let point = view.read(cx).buffer().point();
                (view, point)
            })
            .collect::<Vec<_>>()
    });
    visual.simulate_keystrokes("+");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(matches!(
            shell.active_view().read(cx).buffer().history_query(),
            Some(Query::Log { limit: 2, .. })
        ));
        for (view, point) in before {
            assert_eq!(view.read(cx).buffer().point(), point);
        }
    });
    let old = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    std::fs::rename(repo.0.join(".git"), repo.0.join(".missing-git")).unwrap();
    visual.simulate_keystrokes("g");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            old
        );
    });
    std::fs::rename(repo.0.join(".missing-git"), repo.0.join(".git")).unwrap();
}

#[gpui::test]
fn killed_history_target_and_new_input_cannot_publish_late_results(cx: &mut TestAppContext) {
    let repo = fixture();
    let (mut visual, shell) = open(&repo, cx);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.history_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::MagitLogCurrent);
    until(&mut visual, &shell, |shell| {
        shell
            .buffers
            .iter()
            .any(|slot| slot.buffer.history_query().is_some())
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell
                .buffers
                .iter()
                .position(|slot| slot.buffer.history_query().is_some())
                .unwrap();
            shell.kill_buffer_at(index, window, cx);
        })
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .buffers()
                .all(|buffer| buffer.history_query().is_none())
        );
    });
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.history_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::MagitLogCurrent);
    until(&mut visual, &shell, |shell| {
        shell
            .buffers
            .iter()
            .any(|slot| slot.buffer.history_query().is_some())
    });
    visual.simulate_keystrokes("alt-x");
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_some());
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::FUNDAMENTAL
        );
    });
}

#[gpui::test]
fn fixup_selection_uses_log_keys_and_cancel_never_starts_git(cx: &mut TestAppContext) {
    let repo = fixture();
    repo.write("a", "staged\n");
    repo.git(&["add", "a"]);
    let before = repo.git(&["rev-parse", "HEAD"]);
    let (mut visual, shell) = open(&repo, cx);
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.set_commit_runtime(crate::commit::Runtime::current().unwrap())
        })
    });
    command(&mut visual, &shell, Command::MagitCommitFixup);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_LOG_SELECT
        );
        assert_eq!(shell.read(cx).commit_records().count(), 0);
    });
    visual.simulate_keystrokes("p");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).commit_records().count(), 0);
    });
    visual.simulate_keystrokes("q");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(shell.read(cx).commit_selections.is_empty());
        assert_eq!(shell.read(cx).commit_records().count(), 0);
        assert!(
            shell
                .read(cx)
                .buffers()
                .all(|buffer| buffer.mode() != ModeId::MAGIT_LOG_SELECT)
        );
    });
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(repo.git(&["show", ":a"]), b"staged\n");
}

#[gpui::test]
fn combined_ret_visits_result_and_explicit_choice_visits_parent(cx: &mut TestAppContext) {
    let repo = Fixture::new();
    crate::history::tests::merge(&repo, 2);
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    let revision = visual.read(|cx| shell.read(cx).active_view().clone());
    point(&mut visual, &shell, "++resolved");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "resolved\n"
        )
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&revision, cx).unwrap();
            shell.activate(index, window, cx);
        })
    });
    point(&mut visual, &shell, "++resolved");
    command(&mut visual, &shell, Command::MagitVisitFileVersion);
    visual.simulate_input("parent 2");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert!(buffer.is_read_only());
        assert_eq!(buffer.text().to_string(), "parent 1\n");
    });
}

#[gpui::test]
fn history_export_detaches_before_a_pending_refresh_can_install(cx: &mut TestAppContext) {
    let repo = fixture();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    let source = visual.read(|cx| shell.read(cx).active_view().clone());
    let text = visual.read(|cx| source.read(cx).buffer().text().to_string());
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.history_read_gate = Some(receive)));
    visual.simulate_keystrokes("g");
    let path = repo.0.join("export-log.txt");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&source, path.clone(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        shell
            .buffers
            .iter()
            .any(|slot| slot.buffer.path().as_ref() == Some(&path))
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    visual.read(|cx| {
        let buffer = source.read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::FUNDAMENTAL);
        assert!(buffer.history_query().is_none());
        assert!(buffer.sections().is_none());
        assert!(!buffer.is_read_only());
    });
}

#[gpui::test]
fn a_terminal_fences_an_unrelated_inflight_log_refresh(cx: &mut TestAppContext) {
    let repo = fixture();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    let before = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.history_read_gate = Some(receive)));
    visual.simulate_keystrokes("g");
    repo.write("a", "new terminal\n");
    repo.commit();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let repository = shell.active_view().read(cx).buffer().repository().unwrap();
            let command_buffer = shell.buffers[0].id();
            shell.queue_commit_refresh(&repository, command_buffer, window, cx);
        })
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buffer.text().to_string(), before);
        assert!(buffer.git_is_stale());
    });
    visual.simulate_keystrokes("g");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert_ne!(buffer.text().to_string(), before);
        assert!(!buffer.git_is_stale());
    });
}

#[gpui::test]
fn fixup_merge_range_keeps_upstream_abort_choice_without_rebasing(cx: &mut TestAppContext) {
    let repo = Fixture::new();
    crate::history::tests::merge(&repo, 2);
    repo.write("a", "indexed\n");
    repo.git(&["add", "a"]);
    let base = String::from_utf8(repo.git(&["rev-parse", "HEAD^1^"]))
        .unwrap()
        .trim()
        .to_owned();
    let before = repo.git(&["rev-parse", "HEAD"]);
    let (mut visual, shell) = open(&repo, cx);
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.set_commit_runtime(crate::commit::Runtime::current().unwrap())
        })
    });
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        view.update(cx, |view, cx| {
            let point = view
                .buffer()
                .history_commit_point(Some(&base), false)
                .unwrap();
            view.show_location(point, window, cx);
        });
    });
    command(&mut visual, &shell, Command::MagitCommitFixup);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::CommitTargetMerges { .. })
        ))
    });
    visual.simulate_keystrokes("a");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(shell.read(cx).commit_records().count(), 0);
    });
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(repo.git(&["show", ":a"]), b"indexed\n");
    assert!(!repo.0.join(".git/rebase-merge").exists());
}

#[gpui::test]
fn published_amend_and_commit_menu_cancel_do_not_launch(cx: &mut TestAppContext) {
    let repo = fixture();
    repo.git(&["update-ref", "refs/remotes/origin/master", "HEAD"]);
    let (mut visual, shell) = open(&repo, cx);
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.set_commit_runtime(crate::commit::Runtime::current().unwrap())
        })
    });
    command(&mut visual, &shell, Command::MagitCommit);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::CommitMenu)
        ));
    });
    visual.simulate_keystrokes("ctrl-g");
    command(&mut visual, &shell, Command::MagitCommitAmend);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::CommitPublished { .. })
        ));
        assert_eq!(shell.read(cx).commit_records().count(), 0);
    });
    visual.simulate_keystrokes("n");
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_none());
        assert_eq!(shell.read(cx).commit_records().count(), 0);
    });
}
