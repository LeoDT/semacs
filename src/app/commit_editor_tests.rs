use super::*;

#[path = "commit_revisit_tests.rs"]
mod revisit;
use crate::app::git_test_support::{command, until, wait};
use crate::buffer::{Edit, EditError, EditSource};
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, InputHandler, TestAppContext, VisualTestContext, px, size};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".pi/tmp")
            .join(format!(
                "commit-editor-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("message")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn setup(
    cx: &mut TestAppContext,
) -> (
    Fixture,
    VisualTestContext,
    Entity<AppShell>,
    Entity<BufferView>,
    UnixStream,
) {
    let fixture = Fixture::new();
    fs::write(fixture.path(), "initial\n\n# instructions\n").unwrap();
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    let (request, peer) = EditorRequest::test_pair(fixture.path());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx)
        })
    });
    until(&mut visual, &shell, |shell| {
        !shell.commit_editors.is_empty()
    });
    let editor = visual.read(|cx| shell.read(cx).active_view().clone());
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().mode()),
        ModeId::GIT_COMMIT
    );
    (fixture, visual, shell, editor, peer)
}

fn text(visual: &VisualTestContext, editor: &Entity<BufferView>) -> String {
    visual.read(|cx| editor.read(cx).buffer().text().to_string())
}
fn replace(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    editor: &Entity<BufferView>,
    text: &str,
) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let mut buffer = editor.read(cx).buffer().new_view();
            buffer
                .edit(Edit {
                    snapshot: buffer.snapshot_id(),
                    range: ByteOffset(0)..ByteOffset(buffer.text().len()),
                    text,
                    source: EditSource::Command,
                })
                .unwrap();
            shell.refresh_buffer(buffer.snapshot_id().buffer, cx);
        });
        window.draw(cx).clear(cx);
    });
}
fn gate(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
) -> futures_channel::oneshot::Sender<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.save_gate = Some(receive)));
    send
}
fn pending(peer: &mut UnixStream) {
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
}
fn reply(peer: &mut UnixStream, expected: u8) {
    let mut byte = [99];
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [expected]);
}

#[gpui::test]
fn finish_locks_shared_views_and_waits_for_save_before_reply(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "finished\n\n# instructions\n");
    command(&mut visual, &shell, Command::SplitWindowRight);
    let other = visual.read(|cx| shell.read(cx).active_view().clone());
    let release = gate(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-c ctrl-c");
    pending(&mut peer);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), Some(ENDING));
        assert_eq!(other.read(cx).buffer().edit_inhibition(), Some(ENDING));
        assert!(!editor.read(cx).buffer().is_read_only());
    });
    visual.simulate_input("lost input");
    visual.simulate_keystrokes("enter backspace ctrl-/ ctrl-c ctrl-c ctrl-x ctrl-s ctrl-x ctrl-w");
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_none());
        let mut buffer = editor.read(cx).buffer().new_view();
        assert_eq!(buffer.undo(), Err(EditError::EditingInhibited(ENDING)));
        assert_eq!(buffer.redo(), Err(EditError::EditingInhibited(ENDING)));
        assert_eq!(buffer.text().to_string(), "finished\n\n# instructions\n");
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&other, fixture.0.join("blocked"), window, cx)
        })
    });
    assert!(!fixture.0.join("blocked").exists());
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "finished\n\n# instructions\n"
    );
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.commit_editors.is_empty());
        assert_eq!(shell.panes.leaves(), vec![0, 2]);
        assert!(
            shell
                .buffers
                .iter()
                .all(|slot| slot.buffer.mode() != ModeId::GIT_COMMIT)
        );
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), None);
    });
}

#[gpui::test]
fn cancel_saves_and_kill_command_is_remapped_without_a_kill_prompt(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    visual.simulate_input("q");
    assert!(text(&visual, &editor).starts_with('q'));
    replace(
        &mut visual,
        &shell,
        &editor,
        "canceled\nnonempty second line\n# comment\n",
    );
    let release = gate(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-x k");
    visual.read(|cx| assert!(shell.read(cx).session.is_none()));
    pending(&mut peer);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 1);
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "canceled\nnonempty second line\n# comment\n"
    );
}

#[gpui::test]
fn save_failure_unlocks_and_preserves_editor_for_retry(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "retry\n");
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let index = shell.active_index();
            shell.buffers[index]
                .buffer
                .set_path(fixture.0.join("missing/message"));
        })
    });
    command(&mut visual, &shell, Command::CommitFinish);
    wait(&mut visual, &shell);
    pending(&mut peer);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), None);
        assert_eq!(shell.read(cx).commit_editors.len(), 1);
        assert!(
            shell
                .read(cx)
                .echo
                .as_ref()
                .unwrap()
                .1
                .contains("Save failed")
        );
    });
    visual.simulate_input("can edit ");
    assert!(text(&visual, &editor).contains("can edit "));
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let index = shell.active_index();
            shell.buffers[index].buffer.set_path(fixture.path());
        })
    });
    command(&mut visual, &shell, Command::CommitFinish);
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
}

#[gpui::test]
fn overwrite_confirmation_can_be_declined_or_canceled_and_retried(cx: &mut TestAppContext) {
    for abort in [false, true] {
        let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
        replace(&mut visual, &shell, &editor, "updated\n");
        fs::write(fixture.path(), "changed outside the editor\n").unwrap();
        command(&mut visual, &shell, Command::CommitFinish);
        until(&mut visual, &shell, |shell| {
            shell
                .session
                .as_ref()
                .is_some_and(|session| matches!(session.kind(), ReadKind::ExternalChange(_)))
        });
        visual.read(|cx| assert_eq!(editor.read(cx).buffer().edit_inhibition(), Some(ENDING)));
        if abort {
            visual.simulate_keystrokes("ctrl-g");
        } else {
            visual.simulate_input("no");
            visual.simulate_keystrokes("enter");
        }
        visual.read(|cx| assert_eq!(editor.read(cx).buffer().edit_inhibition(), None));
        pending(&mut peer);
        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            "changed outside the editor\n"
        );
        command(&mut visual, &shell, Command::CommitFinish);
        until(&mut visual, &shell, |shell| shell.session.is_some());
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
        wait(&mut visual, &shell);
        reply(&mut peer, 0);
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "updated\n");
    }
}

#[gpui::test]
fn finish_joins_existing_save_as_queue_without_redirecting_gits_original_path(
    cx: &mut TestAppContext,
) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "older snapshot\n");
    let first = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::SaveBuffer);
    replace(&mut visual, &shell, &editor, "latest snapshot\n");
    let export = fixture.0.join("export");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&editor, export.clone(), window, cx)
        })
    });
    command(&mut visual, &shell, Command::CommitFinish);
    let second = gate(&mut visual, &shell);
    first.send(()).unwrap();
    until(&mut visual, &shell, |_| {
        fs::read_to_string(fixture.path()).unwrap() == "older snapshot\n"
    });
    pending(&mut peer);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().edit_inhibition(), Some(ENDING)));
    second.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    assert_eq!(fs::read_to_string(export).unwrap(), "latest snapshot\n");
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "older snapshot\n"
    );
}

#[gpui::test]
fn style_query_precedes_lock_and_rejection_does_not_finish(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "subject\nbody\n");
    command(&mut visual, &shell, Command::CommitFinish);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session.as_ref().unwrap().kind(),
            &ReadKind::CommitSecondLine
        );
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), None);
    });
    visual.simulate_keystrokes("n");
    pending(&mut peer);
    command(&mut visual, &shell, Command::CommitFinish);
    visual.simulate_keystrokes("y");
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
}

#[gpui::test]
fn history_uses_normal_undo_and_direct_kill_does_not_drop_the_request(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(
        &mut visual,
        &shell,
        &editor,
        "draft\n\n# keep this comment\n",
    );
    command(&mut visual, &shell, Command::CommitPreviousMessage);
    assert_eq!(text(&visual, &editor), "initial\n\n# keep this comment\n");
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).buffer().point(),
            ByteOffset("initial\n".len())
        )
    });
    command(&mut visual, &shell, Command::Undo);
    assert_eq!(text(&visual, &editor), "draft\n\n# keep this comment\n");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&editor, cx).unwrap();
            shell.remove_buffer(index, window, cx);
        })
    });
    pending(&mut peer);
    visual.read(|cx| assert_eq!(shell.read(cx).commit_editors.len(), 1));
    command(&mut visual, &shell, Command::DeleteWindow);
    pending(&mut peer);
    let id = visual.read(|cx| editor.read(cx).buffer().snapshot_id().buffer);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffers.index_of(id).unwrap();
            shell.display_temporary(index, window, cx);
        })
    });
    command(&mut visual, &shell, Command::CommitCancel);
    wait(&mut visual, &shell);
    reply(&mut peer, 1);
}

#[gpui::test]
fn finishing_in_background_preserves_an_unrelated_minibuffer(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "background\n");
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::CommitFinish);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let source = shell.body_panes[&0].active_view().clone();
            shell.begin_read(ReadKind::FindFile, &source, "partial-path", window, cx);
        })
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.session_input(cx).as_deref(), Some("partial-path"));
        assert!(
            shell
                .minibuffer()
                .unwrap()
                .focus_handle(cx)
                .is_focused(window)
        );
        assert!(shell.commit_editors.is_empty());
    });
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn ime_and_script_results_are_rejected_without_replaying_after_failure(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "editable\n");
    visual.update(|_, cx| {
        let mut buffer = editor.read(cx).buffer().new_view();
        let point = editor.read(cx).buffer().point();
        buffer
            .set_mark(if point == ByteOffset(0) {
                ByteOffset(4)
            } else {
                ByteOffset(0)
            })
            .unwrap();
    });
    let captured = visual.read(|cx| editor.read(cx).capture_region().unwrap().0);
    let mut stale_input = crate::view::input_tests::handler(&editor, &visual);
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let index = shell.active_index();
            shell.buffers[index]
                .buffer
                .set_path(fixture.0.join("missing/message"));
        })
    });
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::CommitFinish);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let mut blocked_input = crate::view::input_tests::handler(&editor, &visual);
    visual.update(|window, cx| {
        stale_input.replace_text_in_range(None, "stale", window, cx);
        blocked_input.replace_and_mark_text_in_range(None, "marked", None, window, cx);
        blocked_input.replace_text_in_range(None, "committed", window, cx);
        blocked_input.unmark_text(window, cx);
        assert!(editor.read(cx).display_snapshot().marked_range().is_none());
        assert!(editor.read(cx).capture_region().is_err());
        assert!(
            editor
                .update(cx, |view, cx| view.apply_region_transform(
                    captured,
                    "script result",
                    window,
                    cx
                ))
                .is_err()
        );
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    pending(&mut peer);
    visual.update(|window, cx| {
        blocked_input.replace_text_in_range(None, "late after unlock", window, cx);
        blocked_input.replace_and_mark_text_in_range(None, "late", None, window, cx);
    });
    assert_eq!(text(&visual, &editor), "editable\n");
    visual.simulate_input("fresh");
    assert!(text(&visual, &editor).contains("fresh"));
}

#[gpui::test]
fn reload_preserves_editor_lock_and_default_bindings(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "reload\n");
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::CommitFinish);
    let loaded = gpui_kit::block_on(crate::scripting::load_defaults()).unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_configuration(loaded, window, cx)
        })
    });
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().edit_inhibition(), Some(ENDING)));
    visual.simulate_input("ignored");
    visual.simulate_keystrokes("ctrl-c ctrl-k");
    pending(&mut peer);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
}

#[gpui::test]
fn ordinary_save_stays_editable_and_does_not_reply_to_the_editor(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "saved snapshot\n");
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::SaveBuffer);
    visual.simulate_input("new input ");
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().edit_inhibition(), None));
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    pending(&mut peer);
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "saved snapshot\n"
    );
    visual.read(|cx| assert!(editor.read(cx).buffer().is_modified()));
    command(&mut visual, &shell, Command::CommitCancel);
    wait(&mut visual, &shell);
    reply(&mut peer, 1);
    assert!(
        fs::read_to_string(fixture.path())
            .unwrap()
            .contains("new input ")
    );
}

#[gpui::test]
fn finished_editor_cannot_leave_a_read_session_operating_on_a_dead_target(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "finish\n");
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::CommitFinish);
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    visual.simulate_input("undo");
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    let finished_text = text(&visual, &editor);
    visual.simulate_keystrokes("enter");
    assert_eq!(text(&visual, &editor), finished_text);
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_none());
        assert!(
            shell
                .read(cx)
                .echo
                .as_ref()
                .unwrap()
                .1
                .contains("Read target no longer exists")
        );
    });
}

#[gpui::test]
fn duplicate_requests_share_one_message_buffer_and_are_both_answered(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut first) = setup(cx);
    let (request, mut second) = EditorRequest::test_pair(fixture.path());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx)
        })
    });
    until(&mut visual, &shell, |shell| {
        shell
            .commit_editors
            .values()
            .any(|editor| editor.requests.len() == 2)
    });
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 2));
    replace(&mut visual, &shell, &editor, "shared editor\n");
    command(&mut visual, &shell, Command::CommitCancel);
    wait(&mut visual, &shell);
    reply(&mut first, 1);
    reply(&mut second, 1);
}

#[gpui::test]
fn late_editor_open_keeps_the_request_without_stealing_read_focus(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, mut first) = setup(cx);
    let path = fixture.0.join("late-message");
    fs::write(&path, "late\n").unwrap();
    let (request, mut second) = EditorRequest::test_pair(path);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx);
            shell.begin_read(ReadKind::FindFile, &editor, "in progress", window, cx);
        })
    });
    until(&mut visual, &shell, |shell| shell.commit_editors.len() == 2);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &editor);
        assert_eq!(shell.session_input(cx).as_deref(), Some("in progress"));
        assert!(
            shell
                .minibuffer()
                .unwrap()
                .focus_handle(cx)
                .is_focused(window)
        );
    });
    pending(&mut first);
    pending(&mut second);
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn lost_editor_reply_preserves_the_saved_buffer_and_clears_the_lock(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, peer) = setup(cx);
    drop(peer);
    replace(&mut visual, &shell, &editor, "saved despite lost helper\n");
    command(&mut visual, &shell, Command::CommitFinish);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.commit_editors.is_empty());
        assert!(shell.buffer_index(&editor, cx).is_some());
        assert_eq!(editor.read(cx).buffer().mode(), ModeId::FUNDAMENTAL);
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), None);
        assert!(
            shell
                .echo
                .as_ref()
                .unwrap()
                .1
                .contains("Git editor reply failed")
        );
    });
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "saved despite lost helper\n"
    );
}

#[gpui::test]
fn losing_editor_ownership_cannot_leave_a_shared_buffer_locked(cx: &mut TestAppContext) {
    let (_fixture, mut visual, shell, editor, mut peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "abandoned\n");
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::CommitFinish);
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.commit_editors.clear()));
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().edit_inhibition(), None);
        assert_eq!(editor.read(cx).buffer().mode(), ModeId::FUNDAMENTAL);
    });
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
}

#[test]
fn second_line_style_check_ignores_leading_comments_and_empty_messages() {
    for text in [
        "",
        "\n# instructions\n",
        "subject",
        "subject\n",
        "subject\n\nbody",
        "# instruction\n\nsubject\n# status",
    ] {
        assert!(!needs_second_line_confirmation(text, "#"), "{text:?}");
    }
    for text in [
        "subject\nbody",
        "# instruction\n\nsubject\nbody",
        "subject\n ",
    ] {
        assert!(needs_second_line_confirmation(text, "#"), "{text:?}");
    }
}
