use super::input_tests::{handler, open_buffer, redraw};
use crate::buffer::Buffer;
use crate::diff::{DiffKind, tests::Repository};
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{InputHandler, TestAppContext};
use gpui_kit::{point, px};

fn diff_buffer(repo: &Repository) -> Buffer {
    let data = repo.load(DiffKind::Unstaged);
    let mut buffer = Buffer::diff(data.comparison.clone());
    let request = buffer.begin_diff_request().unwrap().1;
    buffer
        .install_diff(request, data.clone(), data.render().unwrap())
        .unwrap();
    buffer
}

#[test]
fn current_section_fonts_and_geometry_are_view_local_and_recoloring_reuses_them() {
    use crate::theme::{StyleOverride, StyleRole, ThemeSettings};
    use std::sync::Arc;
    crate::test_support::with_text_system(
        Arc::new(crate::test_support::StyledTextSystem::default()),
        |cx| {
            let repo = Repository::new();
            repo.write("a", "old\ncontext\n");
            repo.commit();
            repo.write("a", "new\ncontext\n");
            let buffer = diff_buffer(&repo);
            let other = buffer.new_view();
            let original = buffer.snapshot_id();
            let (mut first_window, first) = open_buffer(buffer, cx);
            let (mut second_window, second) = open_buffer(other, cx);
            let hunk = first_window.read(|cx| {
                first
                    .read(cx)
                    .buffer()
                    .sections()
                    .unwrap()
                    .nodes()
                    .iter()
                    .find(|node| node.key.contains("/@@"))
                    .unwrap()
                    .range
                    .start
            });
            first_window.update(|_, cx| {
                let mut theme = ThemeSettings::default();
                theme.set_style(
                    StyleRole::DiffHunk,
                    StyleOverride::default()
                        .with_text_attributes(StyleRole::DiffHunk, Some("normal"), None, None, None)
                        .unwrap(),
                );
                theme.install(cx);
            });
            redraw(&mut first_window);
            redraw(&mut second_window);
            first_window.update(|window, cx| {
                first.update(cx, |view, cx| view.show_location(hunk, window, cx))
            });
            redraw(&mut first_window);
            redraw(&mut second_window);
            let before = first_window.read(|cx| {
                let a = first.read(cx);
                let b = second.read(cx);
                assert_eq!(a.settings.typography.font_at(hunk.0).bold, Some(true));
                assert_eq!(b.settings.typography.font_at(hunk.0).bold, Some(false));
                assert_eq!(a.buffer().snapshot_id(), original);
                assert_eq!(b.buffer().point(), ByteOffset(0));
                assert!(!a.buffer().is_modified());
                let layout = a.viewport_layout().unwrap();
                let bounds = layout
                    .bounds_for_range(hunk..hunk, a.wrap_index().unwrap())
                    .unwrap();
                assert_eq!(
                    layout.hit_test(bounds.origin + point(px(0.1), px(0.1))),
                    Some(hunk)
                );
                layout.key().clone()
            });
            first_window.update(|_, cx| {
                let mut theme = ThemeSettings::global(cx).clone();
                theme.set_style(
                    StyleRole::DiffHunkHighlight,
                    StyleOverride::new(
                        StyleRole::DiffHunkHighlight,
                        Some("#123456"),
                        Some("#234567"),
                    )
                    .unwrap(),
                );
                theme.install(cx);
            });
            redraw(&mut first_window);
            first_window
                .read(|cx| assert_eq!(first.read(cx).viewport_layout().unwrap().key(), &before));
            first_window.update(|window, cx| {
                window.activate_window();
                first.update(cx, |view, cx| view.restore_input(window, cx));
            });
            first_window.simulate_keystrokes("tab");
            redraw(&mut first_window);
            first_window.read(|cx| {
                let view = first.read(cx);
                assert_eq!(view.buffer().snapshot_id(), original);
                assert_eq!(view.buffer().point(), hunk);
                assert!(
                    view.buffer()
                        .sections()
                        .unwrap()
                        .nodes()
                        .iter()
                        .any(|node| node.range.start == hunk && node.collapsed)
                );
            });
        },
    );
}

#[gpui::test]
fn projected_rows_truncate_and_share_geometry_after_folding(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", format!("{}\n", "中🙂\t".repeat(60)));
    let buffer = diff_buffer(&repo);
    let (mut visual, view) = open_buffer(buffer, cx);
    let (hunk, body) = visual.read(|cx| {
        let sections = view.read(cx).buffer().sections().unwrap();
        let hunk = sections
            .nodes()
            .iter()
            .find(|node| node.key.contains("/@@"))
            .unwrap();
        (hunk.range.start, hunk.body)
    });
    visual.read(|cx| {
        let view = view.read(cx);
        let index = view.wrap_index().unwrap();
        assert_eq!(
            index.row_count(),
            view.buffer().text().max_point().row as usize + 1
        );
        let row = index.row_for_offset(ByteOffset(body.0 + 5)).unwrap();
        assert!(
            index
                .row_for_offset(ByteOffset(row.start.0 + row.row.content_len))
                .is_some()
        );
    });
    visual.update(|window, cx| view.update(cx, |view, cx| view.show_location(hunk, window, cx)));
    redraw(&mut visual);
    let before = visual.read(|cx| view.read(cx).wrap_index().unwrap().row_count());
    let mut stale = handler(&view, &visual);
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        let index = view.wrap_index().unwrap();
        assert!(index.row_count() < before);
        assert!(index.row_for_offset(body).is_none());
        let layout = view.viewport_layout().unwrap();
        let bounds = layout.bounds_for_range(hunk..hunk, index).unwrap();
        assert_eq!(
            layout.hit_test(bounds.origin + point(px(0.1), px(0.1))),
            Some(hunk)
        );
    });
    visual.update(|window, cx| stale.replace_text_in_range(None, "late", window, cx));
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(view.read(cx).buffer().point() >= body);
        assert_ne!(view.read(cx).buffer().point(), body);
    });
    visual.simulate_keystrokes("ctrl-p");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(view.read(cx).buffer().point(), hunk));
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let start = text.find("+中").unwrap();
            let end = text[start..].find('\n').unwrap() + start;
            view.show_location(ByteOffset(end), window, cx);
        })
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.scroll.x > px(0.));
        let caret = view.buffer().point();
        let index = view.wrap_index().unwrap();
        let bounds = view
            .viewport_layout()
            .unwrap()
            .bounds_for_range(caret..caret, index)
            .unwrap();
        assert!(bounds.left() <= view.content_bounds.unwrap().right());
        assert!(view.buffer().is_read_only());
        assert!(!view.buffer().text().to_string().contains("late"));
    });
}

#[gpui::test]
fn asynchronous_long_rows_finish_horizontal_caret_reveal(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n".repeat(100));
    repo.write("z", "old\n");
    repo.commit();
    repo.write("a", "new\n".repeat(100));
    repo.write("z", format!("{}\n", "x".repeat(20_000)));
    let (mut visual, view) = open_buffer(diff_buffer(&repo), cx);
    visual.simulate_keystrokes("alt-> ctrl-b");
    for _ in 0..4 {
        redraw(&mut visual);
    }
    visual.read(|cx| {
        let view = view.read(cx);
        let caret = view.buffer().point();
        assert!(view.scroll.x > px(0.));
        let layout = view.viewport_layout().unwrap();
        let bounds = layout
            .bounds_for_range(caret..caret, view.wrap_index().unwrap())
            .unwrap();
        assert!(bounds.left() >= layout.bounds().left());
        assert!(bounds.left() <= layout.bounds().right());
        assert!(!view.reveal_caret);
    });
}

#[gpui::test]
fn folded_navigation_never_mutates_text_and_text_commands_remain_readonly(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.write("b", "old b\n");
    repo.commit();
    repo.write("a", "new\n");
    repo.write("b", "new b\n");
    let buffer = diff_buffer(&repo);
    let original = buffer.text().to_string();
    let (mut visual, view) = open_buffer(buffer, cx);
    visual.simulate_keystrokes("n n j");
    redraw(&mut visual);
    let file = visual.read(|cx| {
        let view = view.read(cx);
        let point = view.buffer().point();
        assert!(
            view.buffer().sections().unwrap().nodes()
                [view.buffer().sections().unwrap().current(point)]
            .key
            .starts_with("file/")
        );
        point
    });
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    visual.read(|cx| assert!(view.read(cx).buffer().point() > file));
    visual.simulate_keystrokes("^ tab n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        let sections = view.buffer().sections().unwrap();
        assert!(
            sections.nodes()[sections.current(view.buffer().point())]
                .key
                .starts_with("file/\"b")
        );
        assert_eq!(view.buffer().text().to_string(), original);
    });
    visual.simulate_keystrokes("ctrl-d x ctrl-j ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(view.read(cx).buffer().text().to_string(), original));
}
