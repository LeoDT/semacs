use gpui_kit as gpui;
use gpui_kit::{
    Entity, InputHandler, Keystroke, TestAppContext, VisualTestContext, WindowHandle, px, size,
};

use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};
use crate::section::{Section, Sections};
use crate::text::ByteOffset;

fn open_shell(
    cx: &mut TestAppContext,
    text: &str,
) -> (WindowHandle<AppShell>, Entity<AppShell>, VisualTestContext) {
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.to_owned(),
            format: FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(640.), px(360.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (window, shell, visual)
}

fn open_section_shell(
    cx: &mut TestAppContext,
) -> (WindowHandle<AppShell>, Entity<AppShell>, VisualTestContext) {
    cx.update(crate::app::init);
    let text = concat!(
        "Header A\n",
        "  hidden_one match_here\n",
        "Header B\n",
        "  hidden_two match_here\n",
        "Footer visible match_here\n",
    );
    let mut buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.to_owned(),
            format: FileFormat::default(),
        },
    );
    let a_start = 0;
    let a_body = "Header A\n".len();
    let b_start = a_body + "  hidden_one match_here\n".len();
    let b_body = b_start + "Header B\n".len();
    let f_start = b_body + "  hidden_two match_here\n".len();
    let end = text.len();
    let sections = Sections::new(
        &buffer.text(),
        vec![
            Section {
                key: "root".into(),
                parent: None,
                range: ByteOffset(0)..ByteOffset(end),
                body: ByteOffset(0),
                collapsed: false,
                hunk: false,
            },
            Section {
                key: "sec-a".into(),
                parent: Some(0),
                range: ByteOffset(a_start)..ByteOffset(b_start),
                body: ByteOffset(a_body),
                collapsed: true,
                hunk: false,
            },
            Section {
                key: "sec-b".into(),
                parent: Some(0),
                range: ByteOffset(b_start)..ByteOffset(f_start),
                body: ByteOffset(b_body),
                collapsed: true,
                hunk: false,
            },
        ],
    )
    .unwrap();
    buffer.set_read_only(true);
    buffer.install_test_sections(sections);

    let window = cx.open_window(size(px(640.), px(360.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (window, shell, visual)
}

fn dispatch(visual: &mut VisualTestContext, keys: &[&'static str]) {
    for &spec in keys {
        let mut stroke = Keystroke::parse(spec).unwrap_or_else(|_| panic!("invalid key: {spec}"));
        if !stroke.modifiers.control
            && !stroke.modifiers.alt
            && !stroke.modifiers.platform
            && !stroke.modifiers.function
        {
            if stroke.key == "space" {
                stroke.key_char = Some(" ".into());
            } else if stroke.key.chars().count() == 1 {
                stroke.key_char = Some(stroke.key.clone());
            }
        }
        visual.simulate_event(gpui::KeyDownEvent {
            keystroke: stroke,
            is_held: false,
            prefer_character_input: false,
        });
    }
    visual.run_until_parked();
}

#[gpui::test]
fn isearch_forward_repeat_wrap_exit_saves_mark_and_preserves_active_region(
    cx: &mut TestAppContext,
) {
    let (_window, shell, mut visual) = open_shell(cx, "alpha beta alpha gamma");

    // Bare C-s immediately shows "I-search: " prompt (not wiped by deferred Feedback::Clear).
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, _| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.echo_text(), "I-search: ");
    });

    // Type "alpha", repeat to second match, exit with RET.
    dispatch(&mut visual, &["a", "l", "p", "h", "a"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(5));
        assert!(shell.echo_text().starts_with("I-search: alpha"));
    });

    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(16)
        );
    });

    // Wrap pause then wrap around.
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, _| {
        assert!(shell.echo_text().starts_with("Failing I-search: alpha"));
    });
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(5));
        assert!(shell.echo_text().starts_with("Overwrapped I-search: alpha"));
    });

    // Repeat once more to reach second match, then RET exits and saves mark at 0.
    dispatch(&mut visual, &["ctrl-s", "enter"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(!shell.is_isearch_active());
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(buf.point(), ByteOffset(16));
        assert_eq!(buf.mark(), Some(ByteOffset(0)));
        assert!(!buf.mark_active());
        assert_eq!(shell.echo_text(), "Mark saved where search started");
    });

    // Active region is preserved across isearch (`C-t C-s gamma RET C-w`).
    dispatch(
        &mut visual,
        &["ctrl-t", "ctrl-s", "g", "a", "m", "m", "a", "enter"],
    );
    shell.read_with(&visual, |shell, cx| {
        let buf = shell.active_view().read(cx).buffer();
        assert!(buf.mark_active());
        assert_eq!(buf.region(), Some(ByteOffset(16)..ByteOffset(22)));
    });
    dispatch(&mut visual, &["ctrl-w"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "alpha beta alpha"
        );
    });
}

#[gpui::test]
fn isearch_two_stage_abort_delete_char_and_non_isearch_key_exit(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "hello world hello");

    dispatch(&mut visual, &["ctrl-s", "h", "e", "l", "z"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.echo_text().starts_with("Failing I-search: helz"));
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(3));
    });

    // First C-g reverts to last successful state ("hel") and stays in isearch.
    dispatch(&mut visual, &["ctrl-g"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.echo_text(), "I-search: hel");
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(3));
    });

    // Second C-g cancels completely and returns point to origin (0).
    dispatch(&mut visual, &["ctrl-g"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(!shell.is_isearch_active());
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(0));
        assert_eq!(shell.echo_text(), "Quit");
    });

    // Non-isearch key (C-f) exits isearch cleanly and executes forward-char.
    dispatch(&mut visual, &["ctrl-s", "w", "o", "r", "l", "d", "ctrl-f"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(!shell.is_isearch_active());
        // "world" ends at 11, then C-f moves to 12.
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(12)
        );
    });

    // DEL undoes previous isearch input state.
    dispatch(
        &mut visual,
        &["ctrl-a", "ctrl-s", "h", "e", "ctrl-s", "backspace"],
    );
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(2));
    });
    dispatch(&mut visual, &["escape", "escape", "escape"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(!shell.is_isearch_active());
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(0));
    });
}

#[gpui::test]
fn isearch_regexp_incomplete_recovery_yank_ring_and_edit_string(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "foo123 bar456 baz789");

    // C-M-s starts regexp isearch; incomplete "(" recovers when closed.
    dispatch(&mut visual, &["ctrl-alt-s", "(", "b", "a", "r"]);
    shell.read_with(&visual, |shell, _| {
        assert!(shell.echo_text().contains("incomplete input"));
    });
    dispatch(&mut visual, &[")", "\\", "d", "+"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(13)
        );
    });
    dispatch(&mut visual, &["enter"]);
    shell.read_with(&visual, |shell, _| {
        assert!(!shell.is_isearch_active());
    });

    // C-M-s C-s recalls last regexp from regexp-search-ring.
    dispatch(&mut visual, &["alt-<", "ctrl-alt-s", "ctrl-s"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(13)
        );
        assert!(shell.echo_text().contains("(bar)\\d+"));
    });

    // M-e edits the search string in minibuffer, RET resumes isearch.
    dispatch(&mut visual, &["alt-e"]);
    shell.read_with(&visual, |shell, _| {
        assert!(shell.session().is_some());
    });
    dispatch(&mut visual, &["ctrl-a", "ctrl-k"]);
    visual.simulate_input("baz\\d+");
    dispatch(&mut visual, &["enter"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.session().is_none());
        assert!(shell.is_isearch_active());
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(20)
        );
    });
    dispatch(&mut visual, &["enter"]);

    // C-w yanks word at point (downcased), M-s . searches symbol at point.
    dispatch(&mut visual, &["alt-<", "ctrl-s", "ctrl-w"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.echo_text(), "I-search: foo123");
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(6));
    });
    dispatch(&mut visual, &["enter"]);

    // Empty RET enters nonincremental search in minibuffer.
    dispatch(&mut visual, &["alt-<", "ctrl-s", "enter"]);
    shell.read_with(&visual, |shell, _| {
        assert!(shell.session().is_some());
    });
    visual.simulate_input("bar456");
    dispatch(&mut visual, &["enter"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.session().is_none());
        assert!(!shell.is_isearch_active());
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(13)
        );
    });
}

#[gpui::test]
fn isearch_symbol_at_point_quote_newline_del_char_and_boundaries(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "foo_bar foo\nnext_line\nfoo");

    // M-s . starts symbol-at-point search on "foo_bar" (does not match "foo").
    dispatch(&mut visual, &["ctrl-f", "alt-s", "."]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.echo_text(), "Symbol I-search: foo_bar");
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(7));
    });
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, _| {
        assert!(
            shell
                .echo_text()
                .starts_with("Failing symbol I-search: foo_bar")
        );
    });
    dispatch(&mut visual, &["ctrl-g", "ctrl-g"]);

    // C-q C-j searches for a literal newline; M-s M-> jumps to last match; C-M-d deletes last char.
    dispatch(&mut visual, &["ctrl-s", "o", "ctrl-q", "ctrl-j", "n"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.is_isearch_active());
        assert_eq!(shell.echo_text(), "I-search: o^Jn");
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(13)
        );
    });
    dispatch(&mut visual, &["ctrl-alt-d", "ctrl-alt-d"]);
    shell.read_with(&visual, |shell, _| {
        assert_eq!(shell.echo_text(), "I-search: o");
    });
    dispatch(&mut visual, &["alt-s", "alt->"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(25)
        );
    });
    dispatch(&mut visual, &["alt-s", "alt-<"]);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(2));
    });
    dispatch(&mut visual, &["enter"]);
}

#[gpui::test]
fn isearch_temporarily_opens_recollapses_and_keeps_hidden_sections(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_section_shell(cx);

    // Initially both sec-a and sec-b are collapsed.
    shell.read_with(&visual, |shell, cx| {
        let sections = shell.active_view().read(cx).buffer().sections().unwrap();
        assert!(sections.nodes()[1].collapsed);
        assert!(sections.nodes()[2].collapsed);
    });

    // Searching "match_here" opens sec-a temporarily.
    dispatch(
        &mut visual,
        &["ctrl-s", "m", "a", "t", "c", "h", "_", "h", "e", "r", "e"],
    );
    shell.read_with(&visual, |shell, cx| {
        let sections = shell.active_view().read(cx).buffer().sections().unwrap();
        assert!(!sections.nodes()[1].collapsed);
        assert!(sections.nodes()[2].collapsed);
    });

    // Repeating C-s moves into sec-b: sec-a re-collapses immediately, sec-b opens!
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, cx| {
        let sections = shell.active_view().read(cx).buffer().sections().unwrap();
        assert!(sections.nodes()[1].collapsed);
        assert!(!sections.nodes()[2].collapsed);
    });

    // Aborting with C-g returns to 0 and re-collapses sec-b as well.
    dispatch(&mut visual, &["ctrl-g"]);
    shell.read_with(&visual, |shell, cx| {
        assert!(!shell.is_isearch_active());
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(buf.point(), ByteOffset(0));
        let sections = buf.sections().unwrap();
        assert!(sections.nodes()[1].collapsed);
        assert!(sections.nodes()[2].collapsed);
    });

    // Searching again and exiting with RET inside sec-a keeps sec-a open permanently
    // (and pushes "match_here" onto the search ring).
    dispatch(
        &mut visual,
        &[
            "ctrl-s", "m", "a", "t", "c", "h", "_", "h", "e", "r", "e", "enter",
        ],
    );
    shell.read_with(&visual, |shell, cx| {
        let sections = shell.active_view().read(cx).buffer().sections().unwrap();
        assert!(!sections.nodes()[1].collapsed);
        assert!(sections.nodes()[2].collapsed);
    });

    // With M-s i toggled (skip invisible), sec-b is skipped and we jump straight to Footer!
    dispatch(&mut visual, &["alt-<", "ctrl-s", "ctrl-s", "alt-s", "i"]);
    shell.read_with(&visual, |shell, cx| {
        let buf = shell.active_view().read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert!(sections.nodes()[2].collapsed);
        assert!(buf.text().to_string()[..buf.point().0].contains("Header A"));
    });
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, cx| {
        let buf = shell.active_view().read(cx).buffer();
        let sections = buf.sections().unwrap();
        // Skipped collapsed sec-b completely and landed in Footer!
        assert!(sections.nodes()[2].collapsed);
        assert!(buf.text().to_string()[..buf.point().0].contains("Footer visible match_here"));
    });
}

#[gpui::test]
fn isearch_accepts_ime_chinese_input_even_on_read_only_buffer(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "hello 世界 你好 世界");
    let view = shell.read_with(&visual, |shell, _| shell.active_view().clone());
    visual.update(|_, cx| {
        view.update(cx, |view, cx| view.set_read_only(true, cx));
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));

    dispatch(&mut visual, &["ctrl-s"]);
    visual.update(|window, cx| window.draw(cx).clear(cx));

    let mut handler =
        visual.update(|_, cx| view.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        assert!(handler.accepts_text_input(window, cx));
        handler.replace_and_mark_text_in_range(None, "shijie", None, window, cx);
    });
    visual.run_until_parked();
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.echo_text(), "I-search: shijie");
        // Buffer itself must remain completely untouched.
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "hello 世界 你好 世界"
        );
    });

    // D17 IME barrier blocks commands during active preedit.
    dispatch(&mut visual, &["ctrl-s"]);
    shell.read_with(&visual, |shell, _| {
        assert_eq!(
            shell.echo_text(),
            "Input method composition is active; finish it first"
        );
    });

    // Committing Chinese text searches and moves point without mutating buffer.
    let mut handler =
        visual.update(|_, cx| view.update(cx, |view, cx| view.test_input_handler(cx)));
    visual.update(|window, cx| {
        handler.replace_text_in_range(None, "世界", window, cx);
    });
    visual.run_until_parked();
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.echo_text(), "I-search: 世界");
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(buf.point(), ByteOffset("hello 世界".len()));
        assert_eq!(buf.text().to_string(), "hello 世界 你好 世界");
    });
}

#[gpui::test]
fn isearch_toggle_invisible_recollapses_current_section_and_yanks_symbol_with_cmw(
    cx: &mut TestAppContext,
) {
    let (_window, shell, mut visual) = open_section_shell(cx);

    // Match inside collapsed sec-a (temporarily opens sec-a).
    dispatch(
        &mut visual,
        &["ctrl-s", "m", "a", "t", "c", "h", "_", "h", "e", "r", "e"],
    );
    shell.read_with(&visual, |shell, cx| {
        let sections = shell.active_view().read(cx).buffer().sections().unwrap();
        assert!(!sections.nodes()[1].collapsed);
    });

    // Press M-s i while inside sec-a: sec-a must re-collapse and search jumps to visible Footer!
    dispatch(&mut visual, &["alt-s", "i"]);
    shell.read_with(&visual, |shell, cx| {
        let buf = shell.active_view().read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert!(sections.nodes()[1].collapsed);
        assert!(sections.nodes()[2].collapsed);
        assert!(buf.text().to_string()[..buf.point().0].contains("Footer visible match_here"));
    });
    dispatch(&mut visual, &["ctrl-g"]);

    // C-M-w yanks full symbol ("foo_bar") and M-s . on whitespace still allows typing.
    let (_window2, shell2, mut visual2) = open_shell(cx, "   foo_bar baz");
    dispatch(&mut visual2, &["alt-s", "."]);
    shell2.read_with(&visual2, |shell, _| {
        assert!(shell.echo_text().contains("No symbol at point"));
    });
    dispatch(&mut visual2, &["b", "a", "z"]);
    shell2.read_with(&visual2, |shell, cx| {
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(14)
        );
    });
    dispatch(&mut visual2, &["ctrl-g", "ctrl-g"]);
    dispatch(
        &mut visual2,
        &["ctrl-f", "ctrl-f", "ctrl-f", "ctrl-s", "ctrl-alt-w"],
    );
    shell2.read_with(&visual2, |shell, _| {
        assert_eq!(shell.echo_text(), "I-search: foo_bar");
    });
    dispatch(&mut visual2, &["enter"]);
}

#[gpui::test]
fn isearch_quote_and_pending_prefix_handle_prefer_character_input(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "alpha beta alpha");
    dispatch(&mut visual, &["ctrl-s", "ctrl-q"]);
    let mut stroke = Keystroke::parse("a").unwrap();
    stroke.key_char = Some("a".into());
    visual.simulate_event(gpui::KeyDownEvent {
        keystroke: stroke,
        is_held: false,
        prefer_character_input: true,
    });
    visual.run_until_parked();
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.echo_text(), "I-search: a");
        assert_eq!(shell.active_view().read(cx).buffer().point(), ByteOffset(1));
    });

    // Pending `M-s` prefix followed by `w` with prefer_character_input=true toggles word mode.
    dispatch(&mut visual, &["alt-s"]);
    let mut w_stroke = Keystroke::parse("w").unwrap();
    w_stroke.key_char = Some("w".into());
    visual.simulate_event(gpui::KeyDownEvent {
        keystroke: w_stroke,
        is_held: false,
        prefer_character_input: true,
    });
    visual.run_until_parked();
    shell.read_with(&visual, |shell, _| {
        assert!(shell.echo_text().starts_with("Word I-search: a"));
    });
    dispatch(&mut visual, &["enter"]);
}

#[gpui::test]
fn isearch_yank_kill_pulls_external_system_clipboard(cx: &mut TestAppContext) {
    let (_window, shell, mut visual) = open_shell(cx, "first target_symbol last");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("TARGET_SYMBOL".to_owned()));

    dispatch(&mut visual, &["ctrl-s", "ctrl-y"]);
    shell.read_with(&visual, |shell, cx| {
        // `search-upper-case = not-yanks` downcases yanked clipboard text while case-folding.
        assert_eq!(shell.echo_text(), "I-search: target_symbol");
        assert_eq!(
            shell.active_view().read(cx).buffer().point(),
            ByteOffset(19)
        );
        assert_eq!(
            cx.global::<crate::editing::KillRing>()
                .latest()
                .unwrap()
                .to_string(),
            "TARGET_SYMBOL"
        );
    });
    dispatch(&mut visual, &["enter"]);
}
