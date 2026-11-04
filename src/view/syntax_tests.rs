use super::input_tests::{app_shell, handler, open_buffer, redraw};
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};
use crate::theme::{StyleOverride, StyleRole, ThemeSettings};
use gpui_kit::{InputHandler, TestAppContext, px, size};
use std::sync::Arc;

fn buffer(path: &str) -> Buffer {
    Buffer::new(
        Some(path.into()),
        DecodedFile {
            text: "fn main() { let text = \"中🙂\"; }\n".repeat(30),
            format: FileFormat::default(),
        },
    )
}

#[gpui_kit::test]
fn asynchronous_highlights_and_recoloring_preserve_shaping_scroll_and_input_geometry(
    cx: &mut TestAppContext,
) {
    let mut document = buffer("sample.txt");
    let (mut visual, view) = open_buffer(document.new_view(), cx);
    visual.simulate_keystrokes("ctrl-v ctrl-t ctrl-f");
    redraw(&mut visual);
    let before = visual.read(|cx| {
        let view = view.read(cx);
        (
            view.buffer().snapshot_id(),
            view.buffer().point(),
            view.buffer().mark(),
            view.scroll,
            view.cache.clone(),
            view.index.as_ref().unwrap().key().clone(),
            view.geometry_refreshes,
        )
    });
    document.set_path("sample.rs".into());
    view.update(&mut visual, |view, cx| view.synchronize_buffer(cx));
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(!view.read(cx).syntax_ready());
        assert!(view.read(cx).viewport_layout().is_some());
    });
    redraw(&mut visual);
    let syntax = visual.read(|cx| view.read(cx).buffer().syntax_highlights().unwrap());
    visual.update(|window, cx| {
        let mut theme = ThemeSettings::global(cx).clone();
        theme.select("dark").unwrap();
        theme.set_style(
            StyleRole::SyntaxKeyword,
            StyleOverride::new(StyleRole::SyntaxKeyword, Some("#ff0000"), None).unwrap(),
        );
        theme.install(cx);
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        assert_eq!(
            (
                view.buffer().snapshot_id(),
                view.buffer().point(),
                view.buffer().mark(),
                view.scroll,
                view.index.as_ref().unwrap().key().clone(),
                view.geometry_refreshes
            ),
            (before.0, before.1, before.2, before.3, before.5, before.6)
        );
        assert!(!view.buffer().is_modified());
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before.4[row]))
        );
        assert!(Arc::ptr_eq(
            &view.buffer().syntax_highlights().unwrap(),
            &syntax
        ));
    });
    let mut input = handler(&view, &visual);
    visual.update(|window, cx| {
        let range = view.read(cx).display_snapshot().selected_range_utf16();
        assert!(
            input
                .bounds_for_range(range.start.0..range.end.0, window, cx)
                .is_some()
        );
    });
}

#[gpui_kit::test]
fn split_views_share_one_result_through_input_undo_redo_and_language_changes(
    cx: &mut TestAppContext,
) {
    let mut document = buffer("sample.rs");
    let (mut visual, first) = open_buffer(document.new_view(), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    for keys in ["x", "ctrl-/", "ctrl-?"] {
        visual.simulate_keystrokes(keys);
        redraw(&mut visual);
        visual.read(|cx| {
            let a = first.read(cx).buffer().syntax_highlights().unwrap();
            let b = second.read(cx).buffer().syntax_highlights().unwrap();
            assert!(Arc::ptr_eq(&a, &b));
            assert_eq!(a.snapshot, document.snapshot_id());
        });
    }
    document.set_path("plain.txt".into());
    for view in [&first, &second] {
        view.update(&mut visual, |view, cx| view.synchronize_buffer(cx));
    }
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(first.read(cx).buffer().syntax_highlights().is_none());
        assert!(second.read(cx).buffer().syntax_highlights().is_none());
    });
    document.set_path("again.rs".into());
    for view in [&first, &second] {
        view.update(&mut visual, |view, cx| view.synchronize_buffer(cx));
    }
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(Arc::ptr_eq(
            &first.read(cx).buffer().syntax_highlights().unwrap(),
            &second.read(cx).buffer().syntax_highlights().unwrap()
        ))
    });
}

#[gpui_kit::test]
fn preedit_projects_only_committed_colors_and_does_not_start_a_parse(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(buffer("sample.rs"), cx);
    redraw(&mut visual);
    let syntax = visual.read(|cx| view.read(cx).buffer().syntax_highlights().unwrap());
    let mut input = handler(&view, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "中文", None, window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(Arc::ptr_eq(
            &syntax,
            &view.buffer().syntax_highlights().unwrap()
        ));
        let snapshot = view.display_snapshot();
        let marked = snapshot.marked_range().unwrap();
        let colors: Vec<_> = view
            .buffer()
            .syntax_display()
            .unwrap()
            .in_display_range(0..snapshot.text().len(), Some(marked.start.0..marked.end.0))
            .collect();
        assert!(colors.iter().all(|span| span.range.start >= marked.end.0));
        assert_eq!(colors[0].style, StyleRole::SyntaxKeyword);
        assert_eq!(colors[0].range.start, marked.end.0);
    });
    let mut input = handler(&view, &visual);
    visual.update(|window, cx| input.replace_text_in_range(None, "//中\n", window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        let syntax = view.buffer().syntax_highlights().unwrap();
        assert_eq!(syntax.snapshot, view.buffer().snapshot_id());
        assert_eq!(
            syntax.in_range(0..2).next().unwrap().style,
            StyleRole::SyntaxComment
        );
    });
}

#[gpui_kit::test]
fn pending_colors_are_shared_and_projected_before_parse_completion(cx: &mut TestAppContext) {
    for (path, text, token) in [
        ("pending.rs", "fn main() { let n = 42; }\n", "fn"),
        ("pending.md", "```js\nconst n = 42;\n```\n", "const"),
        ("pending.html", "<script>const n = 42;</script>\n", "const"),
    ] {
        let mut document = Buffer::new(
            Some(path.into()),
            DecodedFile {
                text: text.into(),
                format: FileFormat::default(),
            },
        );
        let offset = text.find(token).unwrap();
        let (mut visual, first) = open_buffer(document.new_view(), cx);
        visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
        redraw(&mut visual);
        let second =
            visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
        let original = visual.read(|cx| first.read(cx).buffer().syntax_display().unwrap());
        let mut input = handler(&second, &visual);
        visual.update(|window, cx| {
            input.replace_text_in_range(None, "中", window, cx);
            input.replace_and_mark_text_in_range(None, "拼音", None, window, cx);
            window.draw(cx).clear(cx);
            let a = first.read(cx).buffer().syntax_display().unwrap();
            let view = second.read(cx);
            let b = view.buffer().syntax_display().unwrap();
            assert!(Arc::ptr_eq(&a, &b));
            assert!(!Arc::ptr_eq(&a, &original));
            assert_eq!(b.snapshot, document.snapshot_id());
            assert!(view.syntax_snapshot().is_none());
            assert!(!view.syntax_ready());
            assert!(b.in_range(0.."中".len()).next().is_none());
            assert_eq!(
                b.in_range(offset + 3..offset + 3 + token.len())
                    .next()
                    .unwrap()
                    .style,
                StyleRole::SyntaxKeyword
            );
            let marked = view.display_snapshot().marked_range().unwrap();
            let projected: Vec<_> = b
                .in_display_range(
                    0..view.display_snapshot().text().len(),
                    Some(marked.start.0..marked.end.0),
                )
                .collect();
            assert!(
                projected.iter().all(
                    |span| span.range.end <= marked.start.0 || span.range.start >= marked.end.0
                )
            );
            let token_start = offset + 3 + "拼音".len();
            assert!(
                projected
                    .iter()
                    .any(|span| span.range.contains(&token_start)
                        && span.style == StyleRole::SyntaxKeyword)
            );
            assert!(view.viewport_layout().is_some());
        });
        let mut input = handler(&second, &visual);
        visual.update(|window, cx| {
            input.unmark_text(window, cx);
            for (key, shift) in [("ctrl-/", 3), ("ctrl-/", 0), ("ctrl-?", 3)] {
                window.dispatch_keystroke(gpui_kit::Keystroke::parse(key).unwrap(), cx);
                let view = second.read(cx);
                let display = view.buffer().syntax_display().unwrap();
                assert_eq!(display.snapshot, document.snapshot_id());
                assert!(view.syntax_snapshot().is_none());
                assert_eq!(
                    display
                        .in_range(offset + shift..offset + shift + token.len())
                        .next()
                        .unwrap()
                        .style,
                    StyleRole::SyntaxKeyword
                );
            }
        });
        redraw(&mut visual);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = second.read(cx);
            assert_eq!(view.syntax_snapshot(), Some(document.snapshot_id()));
            assert!(view.syntax_ready());
        });
        document.set_path("pending.txt".into());
        assert!(document.syntax_display().is_none());
    }
}

#[gpui_kit::test]
fn layered_markdown_shares_results_and_preserves_preedit_history_and_geometry(
    cx: &mut TestAppContext,
) {
    let original = "# 中🙂\n\n```html\n<script>const n = 42;</script><style>div { width: 12px; }</style>\n```\n";
    let mut document = Buffer::new(
        Some("sample.md".into()),
        DecodedFile {
            text: original.into(),
            format: FileFormat::default(),
        },
    );
    let (mut visual, first) = open_buffer(document.new_view(), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    redraw(&mut visual);
    let second = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    let syntax = visual.read(|cx| second.read(cx).buffer().syntax_highlights().unwrap());
    let mut input = handler(&second, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "中文", None, window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = second.read(cx);
        assert!(Arc::ptr_eq(
            &syntax,
            &view.buffer().syntax_highlights().unwrap()
        ));
        let marked = view.display_snapshot().marked_range().unwrap();
        assert!(
            view.buffer()
                .syntax_display()
                .unwrap()
                .in_display_range(
                    0..view.display_snapshot().text().len(),
                    Some(marked.start.0..marked.end.0)
                )
                .all(|span| span.range.end <= marked.start.0 || span.range.start >= marked.end.0)
        );
        assert!(Arc::ptr_eq(
            &syntax,
            &first.read(cx).buffer().syntax_highlights().unwrap()
        ));
    });
    visual.update(|window, cx| input.unmark_text(window, cx));
    redraw(&mut visual);
    let cache = visual.read(|cx| second.read(cx).cache.clone());
    visual.update(|_, cx| {
        let mut theme = ThemeSettings::global(cx).clone();
        theme.select("dark").unwrap();
        theme.install(cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = second.read(cx);
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &cache[row]))
        );
        assert!(Arc::ptr_eq(
            &syntax,
            &view.buffer().syntax_highlights().unwrap()
        ));
        assert_eq!(view.buffer().text().to_string(), original);
    });
    for keys in ["x", "ctrl-/", "ctrl-?"] {
        visual.simulate_keystrokes(keys);
        redraw(&mut visual);
        redraw(&mut visual);
        visual.read(|cx| {
            let a = first.read(cx).buffer().syntax_highlights().unwrap();
            let b = second.read(cx).buffer().syntax_highlights().unwrap();
            assert!(Arc::ptr_eq(&a, &b));
            assert_eq!(a.snapshot, document.snapshot_id());
        });
    }
    document.set_path("sample.html".into());
    for view in [&first, &second] {
        view.update(&mut visual, |view, cx| view.synchronize_buffer(cx));
    }
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let a = first.read(cx).buffer().syntax_highlights().unwrap();
        let b = second.read(cx).buffer().syntax_highlights().unwrap();
        assert!(!Arc::ptr_eq(&a, &syntax));
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.snapshot, document.snapshot_id());
    });
}
