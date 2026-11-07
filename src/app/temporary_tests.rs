use crate::app::git_test_support::*;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, px, size};

#[gpui::test]
fn status_splits_below_and_q_buries_without_killing(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
        assert_eq!(*shell.panes.selected(), 1);
        assert!(matches!(
            shell.panes.root(),
            crate::panes::Pane::Split {
                axis: crate::panes::Axis::Vertical,
                ..
            }
        ));
        shell.active_buffer_id()
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0]);
        assert_eq!(shell.active_view(), &origin);
        assert!(shell.buffers.index_of(status).is_some());
        assert_eq!(shell.buffers.ids_in_mru().last(), Some(&status));
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| assert!(origin.read(cx).buffer().text().to_string().starts_with('q')));
}

#[gpui::test]
fn reopening_a_visible_status_reuses_its_window_and_quit_ownership(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &status);
        assert_eq!(shell.panes.leaves().len(), 2);
    });
    command(&mut visual, &shell, Command::MagitModeBuryBuffer);
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves(), vec![0]));
}

#[gpui::test]
fn small_window_restores_nested_replacements_and_cached_origin(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    visual.simulate_resize(size(px(400.), px(100.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0]);
        assert_eq!(
            shell.active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_DIFF
        );
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_STATUS
        )
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| assert_eq!(shell.read(cx).active_view(), &origin));
}

#[gpui::test]
fn quit_removes_only_its_leaf_after_a_manual_split(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::SplitWindowRight);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 2]);
        assert_eq!(*shell.panes.selected(), 0);
        assert_eq!(
            shell.body_panes[&2].active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_STATUS
        );
    });
}

#[gpui::test]
fn retaining_only_the_temporary_window_does_not_let_q_delete_it(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::DeleteOtherWindows);
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![1]);
        assert_eq!(shell.active_index(), 0);
    });
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
}

#[gpui::test]
fn user_buffer_history_prevents_deleting_a_repurposed_window(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let status = shell.active_index();
            let other = shell.create_buffer(Buffer::named("manual"), window, cx);
            shell.activate(other, window, cx);
            shell.activate(status, window, cx);
        });
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
        assert_eq!(
            shell.body_panes[&1]
                .active_view()
                .read(cx)
                .buffer()
                .base_name(),
            "manual"
        );
    });
}

#[gpui::test]
fn late_status_open_cannot_create_a_window_after_origin_changes(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitStatus);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    command(&mut visual, &shell, Command::SplitWindowRight);
    command(&mut visual, &shell, Command::OtherWindow);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_STATUS)
        );
    });
}

#[gpui::test]
fn composing_origin_prevents_a_late_diff_from_creating_windows(cx: &mut TestAppContext) {
    use gpui_kit::InputHandler;
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    let mut input = crate::view::input_tests::handler(&origin, &visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_diff(crate::diff::DiffKind::Unstaged, &origin, window, cx)
        });
        input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0]);
        assert_eq!(shell.active_view(), &origin);
        assert!(!origin.read(cx).git_open_ready());
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
        );
    });
}

#[gpui::test]
fn reload_preserves_quit_restore_and_q_stays_in_the_mode_layer(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let init = repo.0.join("init.js");
    std::fs::write(&init, "semacs.bindKey('q', null, {mode:'magit-status'});").unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.config_path = Some(init.clone());
            let origin = shell.active_view().clone();
            shell.reload_configuration(&origin, window, cx);
        })
    });
    until(&mut visual, &shell, |shell| shell.reload_task.is_none());
    visual.simulate_keystrokes("q");
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves(), vec![0, 1]));
    std::fs::write(&init, "").unwrap();
    command(&mut visual, &shell, Command::ReloadConfiguration);
    until(&mut visual, &shell, |shell| shell.reload_task.is_none());
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    visual.simulate_input("q");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(
            shell
                .minibuffer
                .as_ref()
                .unwrap()
                .view
                .read(cx)
                .editor()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .ends_with('q')
        );
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
    });
    visual.simulate_keystrokes("ctrl-g q");
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves(), vec![0]));
}

#[gpui::test]
fn killed_origin_and_missing_source_never_leave_input_suspended(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::DeleteOtherWindows);
    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.remove_buffer(0, window, cx)));
    visual.simulate_keystrokes("q");
    visual.simulate_input("new scratch");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 1);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "new scratch"
        );
    });
}
