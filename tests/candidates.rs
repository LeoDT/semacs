mod support;

use std::fs;
use std::sync::Arc;

use gpui_kit as gpui;
use gpui_kit::{App, Entity, Keystroke, TestAppContext, VisualTestContext, Window};
use semacs::app::AppShell;
use semacs::completion::CandidateList;
use semacs::text::ByteOffset;
use support::*;

fn candidates(visual: &VisualTestContext, shell: &Entity<AppShell>) -> CandidateList {
    visual.read(|cx| {
        shell
            .read(cx)
            .session()
            .unwrap()
            .candidates()
            .unwrap()
            .clone()
    })
}

fn press(window: &mut Window, cx: &mut App, key: &'static str) {
    let mut stroke = Keystroke::parse(key).unwrap();
    if key.len() == 1 {
        stroke.key_char = Some(key.to_owned());
    }
    window.dispatch_keystroke(stroke, cx);
}

#[gpui::test]
fn file_refresh_keeps_the_single_candidate_and_layout_until_results_arrive(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("candidate-refresh-layout");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    for command in ["ctrl-x ctrl-f", "ctrl-x ctrl-w"] {
        visual.simulate_keystrokes(command);
        visual.simulate_input("a");
        redraw(&mut visual);
        let before = candidates(&visual, &shell);
        assert_eq!(before.items.len(), 1);
        let bounds = visual.read(|cx| body.read(cx).viewport_layout().unwrap().bounds());
        visual.update(|window, cx| {
            press(window, cx, "l");
            press(window, cx, "ctrl-n");
            let list = shell.read(cx).session().unwrap().candidates().unwrap();
            assert!(list.loading);
            assert!(Arc::ptr_eq(&list.items, &before.items));
            assert_eq!(list.selected, before.selected);
            window.refresh();
            window.draw(cx).clear(cx);
            assert_eq!(body.read(cx).viewport_layout().unwrap().bounds(), bounds);
        });
        redraw(&mut visual);
        let after = candidates(&visual, &shell);
        assert!(!after.loading);
        assert_eq!(after.items.len(), 1);
        assert_eq!(after.items[0].candidate, before.items[0].candidate);
        assert_eq!(after.items[0].highlights.len(), 2);
        assert_eq!(
            visual.read(|cx| body.read(cx).viewport_layout().unwrap().bounds()),
            bounds
        );
        visual.simulate_keystrokes("ctrl-g");
    }
}

#[gpui::test]
fn retained_file_candidates_are_not_used_by_pending_actions_or_empty_results(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("candidate-refresh-actions");
    let source = dir.path().join("source.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    fs::write(&beta, b"beta").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    for action in ["tab", "enter", "alt-enter"] {
        visual.simulate_keystrokes("ctrl-x ctrl-f");
        visual.simulate_input("a");
        redraw(&mut visual);
        visual.update(|window, cx| {
            for key in ["ctrl-a", "ctrl-k", "b", action] {
                press(window, cx, key);
            }
            let list = shell.read(cx).session().unwrap().candidates().unwrap();
            assert!(list.loading);
            assert_eq!(list.items[0].candidate.value, "alpha.txt");
            assert_eq!(
                shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
                "b"
            );
            assert_eq!(shell.read(cx).active_view(), &body);
        });
        redraw(&mut visual);
        if action == "tab" {
            assert_eq!(contents(&mut visual, &shell), "beta.txt");
            visual.simulate_keystrokes("ctrl-g");
        } else {
            visual.read(|cx| {
                assert!(shell.read(cx).session().is_none());
                let path = if action == "enter" {
                    beta.clone()
                } else {
                    dir.path().join("b")
                };
                assert_eq!(
                    shell
                        .read(cx)
                        .active_view()
                        .read(cx)
                        .buffer()
                        .path()
                        .as_deref(),
                    Some(path.as_path())
                );
            });
            activate(&mut visual, &shell, 0);
        }
    }
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input("a");
    redraw(&mut visual);
    visual.update(|window, cx| {
        for key in ["ctrl-a", "ctrl-k", "z", "tab"] {
            press(window, cx, key);
        }
        let list = shell.read(cx).session().unwrap().candidates().unwrap();
        assert!(list.loading);
        assert_eq!(list.items[0].candidate.value, "alpha.txt");
    });
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert!(!list.loading);
    assert!(list.items.is_empty());
    assert_eq!(contents(&mut visual, &shell), "z");
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn selection_does_not_edit_input_or_history_and_only_committed_edits_reset_it(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, _) = open(semacs::buffer::Buffer::scratch(), cx);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("forward-");
    visual.simulate_keystrokes("ctrl-n");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "forward-word"
    );
    visual.simulate_keystrokes("ctrl-a ctrl-f ctrl-e");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "forward-word"
    );
    assert_eq!(contents(&mut visual, &shell), "forward-");
    visual.simulate_keystrokes("tab");
    assert_eq!(contents(&mut visual, &shell), "forward-word");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forward-");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "forward-char"
    );
    visual.simulate_keystrokes("up up enter");
    assert!(candidates(&visual, &shell).selected.is_none());
    assert_eq!(contents(&mut visual, &shell), "forward-");
    visual.simulate_keystrokes("alt-enter");
    assert!(visual.read(|cx| shell.read(cx).session().is_some()));
    visual.simulate_keystrokes("down down down down");
    let list = candidates(&visual, &shell);
    assert_eq!(list.selected, Some(list.items.len() - 1));
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn directory_ret_enters_and_complete_directory_preselects_input_without_opening_a_child(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("candidate-directory");
    let source = dir.path().join("source.txt");
    let child = dir.path().join("sub/child.txt");
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(&source, b"source").unwrap();
    fs::write(&child, b"child").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert!(list.selected.is_none());
    assert!(
        list.items
            .iter()
            .any(|item| item.candidate.value == "sub/" && item.candidate.annotation == "directory")
    );
    visual.simulate_input("su");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "sub/");
    assert!(candidates(&visual, &shell).selected.is_none());
    visual.simulate_keystrokes("enter alt-enter");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "sub/");
    assert_eq!(visual.read(|cx| shell.read(cx).active_view().clone()), body);
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "sub/child.txt");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(child.as_path())
        );
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(0));
    });
}

#[gpui::test]
fn file_fuzzy_matching_preserves_directory_prefix_and_smart_case(cx: &mut TestAppContext) {
    let dir = TempDir::new("candidate-path-fuzzy");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("AbcDocument.txt"), b"match").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input("./ad");
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].candidate.value, "./AbcDocument.txt");
    assert_eq!(list.items[0].highlights, vec![0..1, 3..4]);
    visual.simulate_keystrokes("tab");
    assert_eq!(contents(&mut visual, &shell), "./AbcDocument.txt");
    visual.simulate_keystrokes("ctrl-g ctrl-x ctrl-f");
    visual.simulate_input("./AD");
    redraw(&mut visual);
    assert_eq!(candidates(&visual, &shell).items.len(), 1);
    visual.simulate_keystrokes("ctrl-g ctrl-x ctrl-f");
    visual.simulate_input("./Ad");
    redraw(&mut visual);
    assert!(candidates(&visual, &shell).items.is_empty());
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn raw_input_and_input_row_accept_new_paths_even_when_existing_names_match(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("candidate-new-path");
    let source = dir.path().join("source.txt");
    let existing = dir.path().join("alpha.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(&existing, b"existing").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input("alp");
    redraw(&mut visual);
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "alpha.txt"
    );
    visual.simulate_keystrokes("alt-enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(
            view.buffer().path().as_deref(),
            Some(dir.path().join("alp").as_path())
        );
        assert!(view.buffer().text().is_empty());
    });
    assert!(!dir.path().join("alp").exists());
    activate(&mut visual, &shell, 0);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    visual.simulate_input("al");
    redraw(&mut visual);
    visual.simulate_keystrokes("up enter");
    redraw(&mut visual);
    assert_eq!(fs::read(dir.path().join("al")).unwrap(), b"Xsource");
    assert_eq!(fs::read(&existing).unwrap(), b"existing");
    assert_focused(&mut visual, &body);
}

#[gpui::test]
fn selected_existing_save_target_still_requires_explicit_overwrite_confirmation(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("candidate-overwrite");
    let source = dir.path().join("source.txt");
    let existing = dir.path().join("alpha.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(&existing, b"existing").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    visual.simulate_input("aat");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("overwrite?"));
    assert!(visual.read(|cx| shell.read(cx).session().unwrap().candidates().is_none()));
    assert_eq!(fs::read(&existing).unwrap(), b"existing");
    visual.simulate_keystrokes("enter");
    assert_eq!(fs::read(&existing).unwrap(), b"existing");
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn raw_buffer_names_require_an_existing_buffer_and_defaults_remain_available(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, body) = open(semacs::buffer::Buffer::scratch(), cx);
    let second = create(&mut visual, &shell, "other", None);
    activate(&mut visual, &shell, 0);
    visual.simulate_keystrokes("ctrl-x b");
    redraw(&mut visual);
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "*scratch*<2>"
    );
    visual.simulate_input("missing");
    visual.simulate_keystrokes("alt-enter");
    assert!(visual.read(|cx| shell.read(cx).session().is_some()));
    assert_eq!(visual.read(|cx| shell.read(cx).buffer_count()), 2);
    visual.simulate_keystrokes("ctrl-a ctrl-k up enter");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| shell.read(cx).active_index()), second);
    assert_ne!(visual.read(|cx| shell.read(cx).active_view().clone()), body);
    visual.simulate_keystrokes("ctrl-x k");
    redraw(&mut visual);
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        "*scratch*<2>"
    );
    visual.simulate_keystrokes("enter");
    assert_eq!(visual.read(|cx| shell.read(cx).buffer_count()), 1);
}
