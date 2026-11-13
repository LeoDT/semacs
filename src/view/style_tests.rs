use super::PREVIEW_BYTES;
use super::input_tests::{handler, open_buffer, redraw};
use crate::buffer::{Buffer, LineBackground, StyledRange};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use crate::theme::{StyleOverride, StyleRole, ThemeSettings};
use gpui_kit::{InputHandler, TestAppContext, point, px};
use std::sync::Arc;

fn document(path: &str, text: &str) -> Buffer {
    Buffer::new(
        Some(path.into()),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn set_keyword_style(visual: &mut gpui_kit::VisualTestContext, bold: bool) {
    visual.update(|_, cx| {
        let mut theme = ThemeSettings::global(cx).clone();
        theme.set_style(
            StyleRole::SyntaxKeyword,
            StyleOverride::default()
                .with_text_attributes(
                    StyleRole::SyntaxKeyword,
                    Some(if bold { "bold" } else { "normal" }),
                    Some(true),
                    Some(true),
                    Some(true),
                )
                .unwrap(),
        );
        theme.install(cx);
    });
}

#[gpui_kit::test]
fn typography_rebuilds_geometry_without_reparsing_and_colors_reuse_it(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(
        document(
            "styles.rs",
            &"fn main() { let text = \"中🙂\"; }\n".repeat(100),
        ),
        cx,
    );
    let (before, syntax, id) = visual.read(|cx| {
        let view = view.read(cx);
        (
            view.cache.clone(),
            view.buffer().syntax_highlights().unwrap(),
            view.buffer().snapshot_id(),
        )
    });
    set_keyword_style(&mut visual, true);
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.visible_layout_ready());
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| !Arc::ptr_eq(shaped, &before[row]))
        );
        assert!(Arc::ptr_eq(
            &syntax,
            &view.buffer().syntax_highlights().unwrap()
        ));
        assert_eq!(view.buffer().snapshot_id(), id);
        assert!(!view.buffer().is_modified());
        assert_eq!(view.settings.typography.font_at(0).bold, Some(true));
        for row in view.geometry.as_ref().unwrap().rows.iter() {
            assert!(row.width <= view.settings.wrap_width);
        }
    });
    let before = visual.read(|cx| view.read(cx).cache.clone());
    visual.update(|_, cx| {
        let mut theme = ThemeSettings::global(cx).clone();
        theme.set_style(
            StyleRole::SyntaxKeyword,
            StyleOverride::new(StyleRole::SyntaxKeyword, Some("#aabbcc"), Some("#112233")).unwrap(),
        );
        theme.install(cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(
            view.read(cx)
                .cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before[row]))
        )
    });
    let mut input = handler(&view, &visual);
    visual.update(|window, cx| assert!(input.bounds_for_range(0..0, window, cx).is_some()));
}

#[gpui_kit::test]
fn mapped_fonts_keep_local_reflow_during_pending_syntax(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(
        document("styles.rs", &"fn example() { let n = 42; }\n".repeat(2000)),
        cx,
    );
    set_keyword_style(&mut visual, true);
    redraw(&mut visual);
    redraw(&mut visual);
    let mut input = handler(&view, &visual);
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "x", window, cx);
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        assert!(!view.syntax_ready());
        assert!(view.index.as_ref().unwrap().scanned_bytes() < PREVIEW_BYTES);
        assert!(view.index.as_ref().unwrap().is_complete());
        assert_eq!(view.settings.typography.font_at(0).bold, None);
        assert_eq!(view.settings.typography.font_at(1).bold, Some(true));
    });
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.layout_ready());
        assert_eq!(view.settings.typography.font_at(1).bold, None);
        assert!(view.index.as_ref().unwrap().scanned_bytes() < PREVIEW_BYTES);
    });
}

#[gpui_kit::test]
fn syntax_publication_without_emphasis_change_keeps_shaped_rows(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(
        document("styles.rs", &"// note\nfn main() {}\n".repeat(500)),
        cx,
    );
    set_keyword_style(&mut visual, true);
    redraw(&mut visual);
    redraw(&mut visual);
    let mut input = handler(&view, &visual);
    let before = visual.update(|window, cx| {
        input.replace_text_in_range(None, "x", window, cx);
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        assert!(!view.syntax_ready());
        (
            view.cache.clone(),
            view.index.as_ref().unwrap().key().clone(),
        )
    });
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.syntax_ready());
        assert!(view.layout_ready());
        assert_eq!(view.settings.typography.font_at(9).bold, Some(true));
        assert_ne!(view.settings.typography, before.1.settings.typography);
        assert!(view.emphasis_work.is_none());
        assert!(view.emphasis_result.is_none());
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before.0[row]))
        );
    });
}

#[gpui_kit::test]
fn resize_while_emphasis_comparison_is_pending_keeps_paint_layout(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(
        document("styles.rs", &"fn example() { let n = 42; }\n".repeat(200)),
        cx,
    );
    set_keyword_style(&mut visual, true);
    redraw(&mut visual);
    redraw(&mut visual);
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let display = view.display_snapshot().id();
            let pending = view.typography(cx);
            let mut bounds = view.viewport_layout().unwrap().bounds().inset(px(-8.));
            bounds.size.width = (bounds.size.width - px(80.)).max(px(1.));
            // Force the "typography differs" branch with a comparison in flight.
            view.settings.typography = crate::display::Typography::default();
            let task = cx.spawn(async move |_, _| std::future::pending::<()>().await);
            view.emphasis_work = Some(super::EmphasisWork {
                display,
                typography: pending,
                _work: super::BackgroundWork {
                    cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    _task: task,
                },
            });
            view.prepare(bounds, window, cx);
            assert!(view.emphasis_work.is_none());
            assert!(view.viewport_layout().is_some());
            assert!(!view.paint_layout().unwrap().rows.is_empty());
        });
    });
    redraw(&mut visual);
    visual.read(|cx| assert!(view.read(cx).layout_ready()));
}

#[gpui_kit::test]
fn markdown_heading_publication_keeps_rows_when_emphasis_matches(cx: &mut TestAppContext) {
    let text = format!(
        "{}# Heading\n{}",
        "normal paragraph text here\n".repeat(3),
        "plain text line\n".repeat(300),
    );
    let (mut visual, view) = open_buffer(document("notes.md", &text), cx);
    redraw(&mut visual);
    redraw(&mut visual);
    let mut input = handler(&view, &visual);
    let before = visual.update(|window, cx| {
        input.replace_text_in_range(None, "x", window, cx);
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        assert!(!view.syntax_ready());
        assert!((82..90).any(|offset| view.settings.typography.font_at(offset).bold == Some(true)));
        (view.cache.clone(), view.settings.typography.clone())
    });
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(view.syntax_ready());
        assert!(view.layout_ready());
        assert!((82..90).any(|offset| view.settings.typography.font_at(offset).bold == Some(true)));
        assert_ne!(view.settings.typography, before.1);
        assert!(view.emphasis_work.is_none());
        assert!(view.emphasis_result.is_none());
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before.0[row]))
        );
    });
}

#[gpui_kit::test]
fn font_change_preserves_scrolled_text_anchor_and_point(cx: &mut TestAppContext) {
    let (mut visual, view) = open_buffer(
        document(
            "styles.rs",
            &"fn long_name() { let n = \"abcdefghijklmnopqrstuvwxyz\"; }\n".repeat(1000),
        ),
        cx,
    );
    view.update(&mut visual, |view, cx| {
        view.scroll_by(point(px(0.), px(500.)), cx)
    });
    redraw(&mut visual);
    let (anchor, point) = visual.read(|cx| {
        let view = view.read(cx);
        let layout = view.geometry.as_ref().unwrap();
        (layout.rows[0].source.start, view.buffer().point())
    });
    set_keyword_style(&mut visual, true);
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.buffer().point(), point);
        let row = view.index.as_ref().unwrap().row_for_offset(anchor).unwrap();
        assert_eq!(
            view.geometry.as_ref().unwrap().rows[0].source.index,
            row.index
        );
        assert!(view.scroll_anchor.is_none());
    });
}

#[gpui_kit::test]
fn explicit_navigation_cancels_a_background_scroll_anchor(cx: &mut TestAppContext) {
    for command in [
        crate::command::Command::BeginningOfBuffer,
        crate::command::Command::Recenter,
    ] {
        let (mut visual, view) = open_buffer(
            document("styles.rs", &"fn example() { let n = 42; }\n".repeat(2000)),
            cx,
        );
        view.update(&mut visual, |view, cx| {
            view.scroll_by(point(px(0.), px(6000.)), cx)
        });
        redraw(&mut visual);
        set_keyword_style(&mut visual, true);
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(view.read(cx).scroll_anchor.is_some());
            assert!(!view.read(cx).layout_ready());
            view.update(cx, |view, cx| view.execute_command(command, window, cx));
            assert!(view.read(cx).scroll_anchor.is_none());
            window.draw(cx).clear(cx);
        });
        redraw(&mut visual);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.buffer().point(), ByteOffset(0));
            assert_eq!(view.scroll.y, px(0.));
            assert!(view.cursor().is_some());
        });
    }
}

#[test]
fn contextual_expansion_keeps_exact_visible_input_geometry_without_rewrapping() {
    crate::test_support::with_styled_text_system(|cx| {
        let (mut visual, view) = open_buffer(document("plain.txt", &"AV".repeat(100)), cx);
        let first_len = visual.read(|cx| {
            let view = view.read(cx);
            let row = &view.geometry.as_ref().unwrap().rows[0];
            assert!(row.width > view.settings.wrap_width);
            assert_eq!(
                row.source.row.content_len,
                (f32::from(view.settings.wrap_width) / 10.).floor() as usize
            );
            assert_eq!(row.x_for_offset(ByteOffset(2)), Some(px(25.)));
            let regions = view
                .geometry
                .as_ref()
                .unwrap()
                .region_bounds([ByteOffset(2)..ByteOffset(3)]);
            assert_eq!(
                regions[0].left() - view.geometry.as_ref().unwrap().bounds.left(),
                px(25.)
            );
            row.source.row.content_len
        });
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            let bounds = input.bounds_for_range(2..2, window, cx).unwrap();
            let left = view.read(cx).viewport_layout().unwrap().bounds.left();
            assert_eq!(bounds.left() - left, px(25.));
            assert_eq!(
                input.character_index_for_point(
                    point(bounds.left(), bounds.top() + px(4.)),
                    window,
                    cx
                ),
                Some(2)
            );
            input.replace_and_mark_text_in_range(Some(2..2), "x", None, window, cx);
            assert!(input.bounds_for_range(2..2, window, cx).is_none());
        });
        redraw(&mut visual);
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            let bounds = input.bounds_for_range(2..3, window, cx).unwrap();
            let left = view.read(cx).viewport_layout().unwrap().bounds.left();
            assert_eq!(bounds.left() - left, px(25.));
            assert_eq!(bounds.size.width, px(10.));
            assert_eq!(
                view.read(cx)
                    .index
                    .as_ref()
                    .unwrap()
                    .row(0)
                    .unwrap()
                    .row
                    .content_len,
                first_len
            );
        });
    });
}

#[test]
fn styled_geometry_has_font_sensitive_wraps_hit_tests_and_ime_bounds() {
    crate::test_support::with_styled_text_system(|cx| {
        let text = format!("{}\nAV\tb\n", "a".repeat(30));
        let (mut visual, view) = open_buffer(document("plain.txt", &text), cx);
        let plain_rows = visual.read(|cx| view.read(cx).index.as_ref().unwrap().row_count());
        visual.update(|_, cx| {
            let mut theme = ThemeSettings::global(cx).clone();
            theme.set_style(
                StyleRole::Default,
                StyleOverride::default()
                    .with_text_attributes(StyleRole::Default, Some("bold"), Some(true), None, None)
                    .unwrap(),
            );
            theme.install(cx);
        });
        redraw(&mut visual);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = view.read(cx);
            assert!(view.index.as_ref().unwrap().row_count() > plain_rows);
            assert_eq!(
                view.index.as_ref().unwrap().row(0).unwrap().row.content_len,
                (f32::from(view.settings.wrap_width) / 16.).floor() as usize
            );
            let row = view
                .geometry
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.source.start == ByteOffset(31))
                .unwrap();
            assert_eq!(row.x_for_offset(ByteOffset(33)), Some(px(40.)));
            assert_eq!(row.x_for_offset(ByteOffset(34)), Some(px(128.)));
        });
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            for (offset, x) in [(31, 0.), (32, 16.), (33, 40.), (34, 128.), (35, 144.)] {
                let bounds = input.bounds_for_range(offset..offset, window, cx).unwrap();
                let left = view.read(cx).viewport_layout().unwrap().bounds.left();
                assert_eq!(bounds.left() - left, px(x), "offset {offset}");
                assert_eq!(
                    input.character_index_for_point(
                        point(bounds.left(), bounds.top() + px(4.)),
                        window,
                        cx
                    ),
                    Some(offset)
                );
            }
            input.replace_and_mark_text_in_range(Some(33..33), "中", None, window, cx);
            assert!(input.bounds_for_range(33..33, window, cx).is_none());
        });
        redraw(&mut visual);
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            let left = view.read(cx).viewport_layout().unwrap().bounds.left();
            for (offset, x) in [(33, 40.), (34, 56.), (35, 128.)] {
                let bounds = input.bounds_for_range(offset..offset, window, cx).unwrap();
                assert_eq!(bounds.left() - left, px(x), "marked offset {offset}");
            }
        });
    });
}

#[cfg(target_os = "linux")]
#[test]
fn bundled_fonts_keep_styled_glyphs_wraps_and_input_geometry_consistent() {
    use crate::appearance::FontSettings;
    use crate::theme::FontEmphasis;
    use gpui_kit::PlatformTextSystem;
    use gpui_wgpu::CosmicTextSystem;

    let backend = Arc::new(CosmicTextSystem::new_without_system_fonts("Lilex"));
    assert!(backend.all_font_names().is_empty());
    crate::test_support::with_text_system(backend.clone(), |cx| {
        let mut buffer = document("plain.txt", &"aaaa bbbb cccc dddd ".repeat(30));
        buffer
            .set_annotations(
                buffer.snapshot_id(),
                [
                    (5..9, true, false),
                    (10..14, false, true),
                    (15..19, true, true),
                ]
                .into_iter()
                .map(|(range, bold, italic)| StyledRange {
                    range: ByteOffset(range.start)..ByteOffset(range.end),
                    style: StyleOverride::default()
                        .with_text_attributes(
                            StyleRole::Default,
                            Some(if bold { "bold" } else { "normal" }),
                            Some(italic),
                            None,
                            None,
                        )
                        .unwrap(),
                })
                .collect(),
                Vec::new(),
            )
            .unwrap();
        let (mut visual, view) = open_buffer(buffer, cx);
        visual.read(|cx| {
            let view = view.read(cx);
            assert!(view.layout_ready());
            assert_eq!(view.settings.font.family, "Lilex");
            let ids: Vec<_> = [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .map(|(bold, italic)| {
                    backend
                        .font_id(
                            &FontEmphasis {
                                bold: Some(bold),
                                italic: Some(italic),
                            }
                            .font(&FontSettings::default().font()),
                        )
                        .unwrap()
                })
                .collect();
            let rows = &view.geometry.as_ref().unwrap().rows;
            assert!(rows.len() > 1);
            assert!(rows.iter().all(|row| row.width <= view.settings.wrap_width));
            let line = rows[0].parts[0].line.as_ref().unwrap();
            assert!(line.len >= 19);
            for run in &line.runs {
                for glyph in &run.glyphs {
                    let style = match glyph.index {
                        5..=8 => 1,
                        10..=13 => 2,
                        15..=18 => 3,
                        _ => 0,
                    };
                    assert_eq!(run.font_id, ids[style], "glyph {}", glyph.index);
                }
            }
        });
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            for offset in [0, 5, 9, 10, 14, 15, 19] {
                let bounds = input.bounds_for_range(offset..offset, window, cx).unwrap();
                assert_eq!(
                    input.character_index_for_point(
                        point(bounds.left(), bounds.top() + px(4.)),
                        window,
                        cx,
                    ),
                    Some(offset)
                );
            }
            input.replace_and_mark_text_in_range(Some(10..10), "x", None, window, cx);
            assert!(input.bounds_for_range(10..10, window, cx).is_none());
        });
        redraw(&mut visual);
        let mut input = handler(&view, &visual);
        visual.update(|window, cx| {
            let bounds = input.bounds_for_range(10..11, window, cx).unwrap();
            assert_eq!(
                input.character_index_for_point(
                    point(bounds.left(), bounds.top() + px(4.)),
                    window,
                    cx,
                ),
                Some(10)
            );
        });
    });
}

#[gpui_kit::test]
fn line_backgrounds_cover_soft_rows_empty_lines_and_eof_without_relayout(cx: &mut TestAppContext) {
    let mut buffer = document("plain.txt", &format!("{}\n\n", "long text ".repeat(15)));
    let (mut visual, view) = open_buffer(buffer.new_view(), cx);
    let before = visual.read(|cx| view.read(cx).cache.clone());
    buffer
        .set_annotations(
            buffer.snapshot_id(),
            vec![StyledRange {
                range: ByteOffset(0)..ByteOffset(4),
                style: StyleOverride::new(StyleRole::Default, None, Some("#334455")).unwrap(),
            }],
            vec![LineBackground {
                rows: 0..3,
                color: gpui_kit::rgb(0x112233).into(),
            }],
        )
        .unwrap();
    view.update(&mut visual, |view, cx| view.synchronize_buffer(cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert!(
            view.cache
                .iter()
                .all(|(row, shaped)| Arc::ptr_eq(shaped, &before[row]))
        );
        let layout = view.geometry.as_ref().unwrap();
        let annotations = buffer.annotations().unwrap();
        let backgrounds = layout.line_backgrounds(&annotations, &buffer.text(), None);
        assert_eq!(backgrounds.len(), layout.rows.len());
        assert!(backgrounds.len() > 3);
        assert!(
            backgrounds
                .iter()
                .all(|(bounds, _)| bounds.left() == layout.bounds.left()
                    && bounds.size.width == layout.bounds.size.width
                    && bounds.size.height == layout.key.settings.line_height)
        );
        assert!(
            layout
                .rows
                .iter()
                .any(|row| row.source.row.content_len == 0)
        );
        assert!(!buffer.is_modified());
    });
}
