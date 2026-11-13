mod support;

use std::fs;
use std::sync::Arc;

use gpui_kit as gpui;
use gpui_kit::{App, Entity, Keystroke, TestAppContext, VisualTestContext, Window};
use semacs::app::AppShell;
use semacs::completion::{AnnotationPlacement, CandidateList};
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
    let bo = dir.path().join("bo.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    fs::write(&bo, b"bo").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    for action in ["tab", "enter", "alt-enter"] {
        visual.simulate_keystrokes("ctrl-x ctrl-f");
        clear_input(&mut visual);
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
            assert_eq!(contents(&mut visual, &shell), "bo.txt");
            visual.simulate_keystrokes("ctrl-g");
        } else {
            visual.read(|cx| {
                assert!(shell.read(cx).session().is_none());
                let path = if action == "enter" {
                    bo.clone()
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
    clear_input(&mut visual);
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

fn candidate_advance(visual: &mut VisualTestContext) -> gpui_kit::Pixels {
    visual.update(|window, cx| {
        let fonts = cx.global::<semacs::appearance::FontSettings>();
        let font_id = window.text_system().resolve_font(&fonts.font());
        window
            .text_system()
            .ch_advance(font_id, fonts.size())
            .unwrap()
    })
}

#[gpui::test]
fn file_candidates_start_the_annotation_at_the_marginalia_column(cx: &mut TestAppContext) {
    let dir = TempDir::new("candidate-annotation-column");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("a.txt"), b"a").unwrap();
    // Exactly twenty cells stays in the twenty cell step; the "+ 1" of an align
    // marker would widen it to thirty.
    fs::write(dir.path().join("01234567890123456789"), b"b").unwrap();
    let (mut visual, _shell, _) = open(buffer_at(&source, "source"), cx);
    let advance = candidate_advance(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    redraw(&mut visual);
    let row = visual.debug_bounds("candidate-row-0").unwrap();
    let label = visual.debug_bounds("candidate-label-0").unwrap();
    let annotation = visual.debug_bounds("candidate-annotation-0").unwrap();
    assert_eq!(label.left() - row.left(), gpui_kit::px(12.));
    assert_eq!(label.size.width, advance * 20.);
    assert_eq!(annotation.left(), label.right());
    visual.simulate_keystrokes("ctrl-g");

    // Other candidate kinds keep the annotation at the right edge.
    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    let label = visual.debug_bounds("candidate-label-0").unwrap();
    let annotation = visual.debug_bounds("candidate-annotation-0").unwrap();
    assert_eq!(annotation.left() - label.right(), gpui_kit::px(12.));
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn the_annotation_column_only_measures_the_visible_page(cx: &mut TestAppContext) {
    let dir = TempDir::new("candidate-annotation-page");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    for index in 0..20 {
        fs::write(dir.path().join(format!("f{index:02}.txt")), b"x").unwrap();
    }
    fs::write(
        dir.path()
            .join("a-name-that-is-much-too-long-to-be-visible"),
        b"x",
    )
    .unwrap();
    let (mut visual, _shell, _) = open(buffer_at(&source, "source"), cx);
    let advance = candidate_advance(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    redraw(&mut visual);
    let label = visual.debug_bounds("candidate-label-0").unwrap();
    assert_eq!(label.size.width, advance * 20.);
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn accepted_file_inputs_rank_by_history_and_annotate_at_a_column(cx: &mut TestAppContext) {
    let dir = TempDir::new("candidate-file-history");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    for name in ["alpha.txt", "alpine.txt", "beta.txt", "zeta.txt"] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    redraw(&mut visual);
    // Without history, length then name puts `beta.txt` before `zeta.txt`.
    assert_eq!(
        candidates(&visual, &shell).items[0].candidate.value,
        "beta.txt"
    );

    visual.simulate_input("zeta");
    redraw(&mut visual);
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
                .display_name(),
            "zeta.txt"
        );
    });

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert_eq!(list.items[0].candidate.value, "zeta.txt");
    assert_eq!(list.annotation, AnnotationPlacement::MarginaliaLeft);
    assert!(list.items[0].candidate.annotation.starts_with('-'));
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn only_accepted_file_answers_enter_the_file_history(cx: &mut TestAppContext) {
    let dir = TempDir::new("candidate-file-history-record");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("beta.txt"), b"beta").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    let history = |visual: &VisualTestContext| {
        visual.read(|cx| {
            cx.global::<semacs::file_history::FileHistory>()
                .entries()
                .to_vec()
        })
    };

    // A directory answer keeps the session open instead of accepting a file.
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    visual.simulate_input("sub");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_some()));
    assert!(history(&visual).is_empty());
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    assert!(history(&visual).is_empty(), "cancelling records nothing");

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    clear_input(&mut visual);
    visual.simulate_input("beta");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_eq!(history(&visual), ["beta.txt"]);

    visual.simulate_keystrokes("ctrl-x ctrl-w");
    clear_input(&mut visual);
    visual.simulate_input("out.txt");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_eq!(history(&visual), ["out.txt", "beta.txt"]);
    assert!(dir.path().join("out.txt").is_file());
}

#[gpui::test]
fn selection_does_not_edit_input_or_history_and_only_committed_edits_reset_it(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, _) = open(semacs::buffer::Buffer::scratch(), cx);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("forward-");
    let list = candidates(&visual, &shell);
    let first = list.items[0].candidate.value.clone();
    let second = list.items[1].candidate.value.clone();
    visual.simulate_keystrokes("ctrl-n");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        second
    );
    visual.simulate_keystrokes("ctrl-a ctrl-f ctrl-e");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        second
    );
    assert_eq!(contents(&mut visual, &shell), "forward-");
    visual.simulate_keystrokes("tab");
    assert_eq!(contents(&mut visual, &shell), second);
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forward-");
    assert_eq!(
        candidates(&visual, &shell)
            .selected_candidate()
            .unwrap()
            .value,
        first
    );
    visual.simulate_keystrokes("up up enter");
    assert!(candidates(&visual, &shell).selected.is_none());
    assert_eq!(contents(&mut visual, &shell), "forward-");
    visual.simulate_keystrokes("alt-enter");
    assert!(visual.read(|cx| shell.read(cx).session().is_some()));
    for _ in 0..candidates(&visual, &shell).items.len() {
        visual.simulate_keystrokes("down");
    }
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
    clear_input(&mut visual);
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert!(list.selected.is_none());
    assert!(list.items.iter().any(|item| item.candidate.value == "sub/"
        && item.candidate.directory
        && item.candidate.annotation.starts_with('d')));
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
    clear_input(&mut visual);
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
    clear_input(&mut visual);
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
    clear_input(&mut visual);
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
    clear_input(&mut visual);
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

#[gpui::test]
fn find_file_prefills_the_buffer_directory_and_lists_its_children(cx: &mut TestAppContext) {
    let dir = TempDir::new("prefill-find");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    let prefix = semacs::files::directory_input(dir.path());
    assert_eq!(prompt(&mut visual, &shell), "Find file: ");
    assert_eq!(contents(&mut visual, &shell), prefix);
    let mut values: Vec<_> = candidates(&visual, &shell)
        .items
        .iter()
        .map(|item| item.candidate.value.to_string())
        .collect();
    values.sort();
    assert_eq!(
        values,
        vec![
            format!("{prefix}alpha.txt"),
            format!("{prefix}source.txt"),
            format!("{prefix}sub/")
        ]
    );

    visual.simulate_input("al");
    redraw(&mut visual);
    let list = candidates(&visual, &shell);
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].candidate.value, format!("{prefix}alpha.txt"));
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
            Some(dir.path().join("alpha.txt").as_path())
        );
    });
}

#[gpui::test]
fn ret_on_the_prefilled_directory_keeps_the_find_file_session(cx: &mut TestAppContext) {
    let dir = TempDir::new("prefill-directory-ret");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    let (mut visual, shell, body) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    let editor = visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone()
    });
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.session().is_some());
        assert_eq!(shell.buffer_count(), 1);
        assert_eq!(shell.active_view(), &body);
        assert_eq!(
            editor.read(cx).message(),
            Some("Select a file or enter a file name")
        );
        assert_eq!(
            shell.minibuffer().unwrap().read(cx).contents(cx),
            semacs::files::directory_input(dir.path())
        );
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
}
