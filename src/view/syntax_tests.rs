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
        let colors: Vec<_> = syntax
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
