use crate::theme::{StyleOverride, StyleRole, ThemeSettings};
use crate::view::input_tests::{open, redraw};
use gpui_kit as gpui;
use gpui_kit::{InputHandler, TestAppContext};
use std::sync::Arc;

#[gpui::test]
fn recoloring_preserves_document_shaping_scroll_and_input_geometry(cx: &mut TestAppContext) {
    let (mut visual, view) = open(&"line 中🙂\ttext\n".repeat(1000), cx);
    visual.simulate_keystrokes("ctrl-v ctrl-t ctrl-f");
    redraw(&mut visual);
    let before = visual.read(|cx| {
        let view = view.read(cx);
        (
            view.buffer().snapshot_id(),
            view.buffer().point(),
            view.buffer().mark(),
            view.buffer().is_modified(),
            view.scroll,
            view.index.as_ref().unwrap().key().clone(),
            view.cache.clone(),
            view.geometry_refreshes,
        )
    });
    visual.update(|window, cx| {
        let mut settings = ThemeSettings::global(cx).clone();
        settings.select("dark").unwrap();
        settings.install(cx);
        window.draw(cx).clear(cx);
    });
    visual.read(|cx| {
        let view = view.read(cx);
        assert_eq!(
            (
                view.buffer().snapshot_id(),
                view.buffer().point(),
                view.buffer().mark(),
                view.buffer().is_modified(),
                view.scroll,
                view.index.as_ref().unwrap().key().clone()
            ),
            (before.0, before.1, before.2, before.3, before.4, before.5)
        );
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before.6[row]))
        );
        assert_eq!(view.geometry_refreshes, before.7);
    });
    visual.update(|window, cx| {
        let mut settings = ThemeSettings::global(cx).clone();
        settings.set_style(
            StyleRole::Cursor,
            StyleOverride::new(StyleRole::Cursor, None, Some("#ffcc66")).unwrap(),
        );
        settings.install(cx);
        window.draw(cx).clear(cx);
    });
    visual.read(|cx| {
        assert!(
            view.read(cx)
                .cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before.6[row]))
        );
    });
}

#[gpui::test]
fn oversized_grapheme_remains_painted_and_queryable_on_the_first_recolored_frame(
    cx: &mut TestAppContext,
) {
    let text = format!("a{}", "\u{301}".repeat(5000));
    let (mut visual, view) = open(&text, cx);
    let rows = visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.layout_ready());
        assert_eq!(view.geometry.as_ref().unwrap().rows.len(), 1);
        assert!(
            view.geometry.as_ref().unwrap().rows[0]
                .source
                .row
                .content_len
                > super::SYNC_SHAPE_BYTES
        );
        view.cache.clone()
    });
    for name in ["dark", "light"] {
        visual.update(|window, cx| {
            let mut settings = ThemeSettings::global(cx).clone();
            settings.select(name).unwrap();
            settings.install(cx);
            window.draw(cx).clear(cx);
            let mut input = crate::platform::BufferInputHandler {
                view: view.downgrade(),
                token: view.read(cx).target.token(),
                registration: view.read(cx).registration,
            };
            let view = view.read(cx);
            assert!(view.layout_ready());
            assert_eq!(view.geometry.as_ref().unwrap().rows.len(), 1);
            assert!(view.shape_work.is_none());
            assert!(
                view.cache
                    .iter()
                    .all(|(row, shaped)| Arc::ptr_eq(shaped, &rows[row]))
            );
            let cursor = view.cursor().unwrap();
            assert!(input.bounds_for_range(0..1, window, cx).is_some());
            assert!(
                input
                    .character_index_for_point(cursor.bounds.origin, window, cx)
                    .is_some()
            );
        });
    }
}
