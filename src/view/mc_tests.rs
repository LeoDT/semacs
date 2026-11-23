use gpui_kit as gpui;
use gpui_kit::{
    Entity, Focusable, InputHandler, Modifiers, MouseButton, MouseDownEvent, TestAppContext,
    VisualTestContext, point, px, size,
};

use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};
use crate::platform::BufferInputHandler;
use crate::text::ByteOffset;
use crate::theme::{StyleRole, ThemeSettings};
use crate::view::{BufferView, decoration::TextStyle};

fn open_shell(
    text: &str,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(640.), px(320.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let shell = window.entity(&visual).unwrap();
    let editor = shell.read_with(&visual, |shell, _| shell.active_view().clone());
    visual.update(|window, cx| {
        editor.read(cx).focus_handle(cx).focus(window, cx);
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
    (visual, shell, editor)
}

fn redraw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
}

#[gpui::test]
fn default_keybindings_mx_and_ret_exit_work_end_to_end(cx: &mut TestAppContext) {
    let (mut visual, _, editor) = open_shell("foo 1 foo 2 foo 3 foo", cx);

    // Select first "foo", mark next twice with C->, backspace all active regions, type "bar".
    visual.simulate_keystrokes("ctrl-t alt-f ctrl-> ctrl->");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.cursor_count(), 3);
        assert_eq!(view.cursors().len(), 3);
        assert_eq!(view.region_bounds().len(), 3);
    });

    // Skip furthest via M-x mc/skip-to-next-like-this (moves 3rd cursor from 3rd foo to 4th foo).
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("mc/skip-to-next-like-this");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.cursor_count(), 3);
        assert_eq!(
            view.target.secondary_cursors(),
            vec![ByteOffset(9), ByteOffset(21)]
        );
    });

    visual.simulate_keystrokes("backspace");
    visual.simulate_input("bar");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).buffer().text().to_string(),
            "bar 1 bar 2 foo 3 bar"
        );
    });

    // Single undo reverts all three "bar", second undo restores all three "foo".
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().text().to_string(), " 1  2 foo 3 ");
    });
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).buffer().text().to_string(),
            "foo 1 foo 2 foo 3 foo"
        );
    });

    // Upstream default: RET exits multiple-cursors mode.
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 1);
    });

    // C-c C-< marks all occurrences.
    visual.simulate_keystrokes("alt-< ctrl-t alt-f ctrl-c ctrl-<");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 4);
    });
}

#[gpui::test]
fn ctrl_shift_c_edit_lines_and_ctrl_shift_click_toggle_cursors(cx: &mut TestAppContext) {
    let (mut visual, _, editor) = open_shell("alpha\nbeta\ngamma", cx);

    // Mark from line 1 col 0 to line 3 col 1 and invoke C-S-c C-S-c (mc/edit-lines).
    visual.simulate_keystrokes("ctrl-t ctrl-n ctrl-n ctrl-f ctrl-shift-c ctrl-shift-c");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 3);
    });
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_input("> ");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).buffer().text().to_string(),
            "> alpha\n> beta\n> gamma"
        );
    });

    // Plain mouse click exits multiple-cursors mode.
    let bounds = editor.read_with(&visual, |view, _| view.viewport_layout().unwrap().bounds());
    let line_height = editor.read_with(&visual, |view, _| view.settings.line_height);
    let line1_click = point(bounds.left() + px(2.), bounds.top() + px(4.));
    let line2_click = point(bounds.left() + px(2.), bounds.top() + line_height + px(4.));
    visual.simulate_mouse_down(line1_click, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(line1_click, MouseButton::Left, Modifiers::default());
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 1);
    });

    // C-S-<mouse-1> adds a cursor at line 2, and clicking line 1 (now a fake cursor) removes it.
    let ctrl_shift = Modifiers {
        control: true,
        shift: true,
        ..Modifiers::default()
    };
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.mouse_down(
                &MouseDownEvent {
                    button: MouseButton::Left,
                    position: line2_click,
                    modifiers: ctrl_shift,
                    click_count: 1,
                    first_mouse: false,
                },
                window,
                cx,
            );
        });
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 2);
    });

    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.mouse_down(
                &MouseDownEvent {
                    button: MouseButton::Left,
                    position: line1_click,
                    modifiers: ctrl_shift,
                    click_count: 1,
                    first_mouse: false,
                },
                window,
                cx,
            );
        });
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 1);
    });
}

#[gpui::test]
fn ime_composition_and_commit_applies_to_all_cursors_as_one_undo_group(cx: &mut TestAppContext) {
    let (mut visual, _, editor) = open_shell("a\nb\nc", cx);
    visual.simulate_keystrokes("ctrl-> ctrl->");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 3);
    });

    let mut input: BufferInputHandler =
        visual.update(|_, cx| editor.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "zh", None, window, cx);
    });
    redraw(&mut visual);
    // Preedit only projects at primary cursor; committed text is still untouched.
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "a\nb\nc");
        assert!(view.target.input().is_composing());
    });

    // Committing IME text via replace_text_in_range inserts at all 3 cursors.
    let mut input: BufferInputHandler =
        visual.update(|_, cx| editor.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "中", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().text().to_string(), "中a\n中b\n中c");
    });

    // One undo reverts all three Chinese insertions.
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().text().to_string(), "a\nb\nc");
    });

    // Unmark commits preedit to all cursors as well.
    let mut input: BufferInputHandler =
        visual.update(|_, cx| editor.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "文", None, window, cx);
        input.unmark_text(window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer().text().to_string(), "文a\n文b\n文c");
    });

    // When all fake cursors precede the primary cursor (added upward via C-<),
    // committing IME preedit keeps DisplayChange valid for incremental reflow.
    let (mut visual, _, editor) = open_shell("a\nb\nc", cx);
    visual.simulate_keystrokes("alt-> ctrl-< ctrl-<");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(editor.read(cx).cursor_count(), 3);
    });
    let mut input: BufferInputHandler =
        visual.update(|_, cx| editor.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        input.replace_text_in_range(None, "你", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "a你\nb你\nc你");
        assert!(view.layout_ready());
    });
}

#[gpui::test]
fn split_views_keep_cursors_view_local_and_isearch_exits_mc(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open_shell("foo\nfoo", cx);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = shell.read_with(&visual, |shell, _| shell.active_view().clone());
    assert_ne!(first.entity_id(), second.entity_id());

    // Enter multiple-cursors mode in the active pane; the other pane stays single-cursor.
    visual.simulate_keystrokes("ctrl->");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(second.read(cx).cursor_count(), 2);
        assert_eq!(first.read(cx).cursor_count(), 1);
    });

    // Editing in the active pane updates the shared text while keeping the other pane single-cursor.
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().text().to_string(), "Xfoo\nXfoo");
        assert_eq!(first.read(cx).cursor_count(), 1);
    });

    // Starting isearch automatically exits multiple-cursors mode.
    visual.simulate_keystrokes("ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(second.read(cx).cursor_count(), 1);
    });
}

#[test]
fn decoration_runs_invert_foreground_for_multiple_cursors_on_same_row() {
    let theme = ThemeSettings::default();
    let style = TextStyle::new(&theme, None, None, 0);
    let runs = style.runs(0..10, &[1..2, 5..6, 8..9]);
    let mut offset = 0;
    for run in runs {
        for byte in offset..offset + run.len as usize {
            let expected = if matches!(byte, 1 | 5 | 8) {
                StyleRole::Cursor
            } else {
                StyleRole::Default
            };
            assert_eq!(run.color, theme.style(expected).foreground, "byte {byte}");
        }
        offset += run.len as usize;
    }
    assert_eq!(offset, 10);
}
