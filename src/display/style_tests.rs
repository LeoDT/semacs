use super::*;
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};
use crate::input::InputSession;
use crate::theme::{FontEmphasis, StyleOverride, StyleRole, ThemeSettings};
use gpui_kit::{AppContext, FontStyle, FontWeight, TestAppContext, font};
use unicode_segmentation::UnicodeSegmentation;

fn settings() -> LayoutSettings {
    LayoutSettings {
        font: font("monospace"),
        font_size: px(16.),
        line_height: px(24.),
        wrap_width: px(90.),
        tab_size: 8,
        typography: Typography::default(),
        visible_lines: None,
    }
}

#[test]
fn projected_logical_rows_handle_unicode_eof_and_hidden_offsets_without_soft_wrapping() {
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: "head\nhidden\n中🙂".into(),
            format: FileFormat::default(),
        },
    );
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let mut settings = settings();
    settings.visible_lines = Some(Arc::from([
        ByteOffset(0)..ByteOffset(5),
        ByteOffset(12)..ByteOffset(19),
    ]));
    let system = Arc::new(TextSystem::new(Arc::new(
        crate::test_support::StyledTextSystem::default(),
    )));
    let mut metrics = FontMeasure::new(system, settings.clone());
    let index = WrapJob::new(snapshot, settings, None, None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(index.row_count(), 2);
    assert_eq!(index.row(1).unwrap().row.content_len, 7);
    assert!(index.row_for_offset(ByteOffset(8)).is_none());
    assert_eq!(index.row_for_offset(ByteOffset(19)).unwrap().index, 1);
}

#[test]
fn named_range_fonts_follow_theme_without_recreating_the_range_table() {
    let mut theme = ThemeSettings::default();
    let named = Arc::from([(ByteOffset(0)..ByteOffset(4), StyleRole::DiffHunk)]);
    let make = |theme: &ThemeSettings| {
        Typography::new(theme, None, None, 0, Arc::from([]))
            .with_named_ranges(Arc::clone(&named), theme)
    };
    let first = make(&theme);
    assert_eq!(first.font_at(0).bold, Some(true));
    theme.set_style(
        StyleRole::DiffHunk,
        StyleOverride::new(StyleRole::DiffHunk, Some("#123456"), None).unwrap(),
    );
    assert_eq!(first, make(&theme));
    theme.set_style(
        StyleRole::DiffHunk,
        StyleOverride::default()
            .with_text_attributes(StyleRole::DiffHunk, Some("normal"), None, None, None)
            .unwrap(),
    );
    let changed = make(&theme);
    assert_ne!(first, changed);
    assert_eq!(changed.font_at(0).bold, Some(false));
}

#[test]
fn wrapping_uses_font_sensitive_grapheme_advances_not_contextual_fitting() {
    use crate::test_support::StyledTextSystem;
    let system = Arc::new(TextSystem::new(Arc::new(StyledTextSystem::default())));
    let wrap = |text: &str, width: f32, typography: Typography| {
        let buffer = Buffer::new(
            None,
            DecodedFile {
                text: text.into(),
                format: FileFormat::default(),
            },
        );
        let snapshot = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let mut settings = settings();
        settings.wrap_width = px(width);
        settings.typography = typography;
        let mut metrics = FontMeasure::new(system.clone(), settings.clone());
        let index = WrapJob::new(snapshot, settings, None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        (index, metrics)
    };
    let (expanded, mut metrics) = wrap("AV", 22., Typography::default());
    assert_eq!(metrics.width("A") + metrics.width("V"), px(20.));
    let runs = metrics.runs("AV", 0);
    assert_eq!(
        metrics
            .system
            .layout_line("AV", metrics.settings.font_size, &runs, None)
            .width,
        px(25.)
    );
    assert_eq!(expanded.row_count(), 1);
    assert_eq!(expanded.row(0).unwrap().row.content_len, 2);
    let (contracted, _) = wrap("fix", 25., Typography::default());
    assert_eq!(contracted.row_count(), 2);
    assert_eq!(contracted.row(0).unwrap().row.content_len, 2);
    for (weight, italic, advance) in [
        (None, None, 10.),
        (Some("bold"), None, 14.),
        (None, Some(true), 12.),
        (Some("bold"), Some(true), 16.),
    ] {
        let mut theme = ThemeSettings::default();
        theme.set_style(
            StyleRole::Default,
            StyleOverride::default()
                .with_text_attributes(StyleRole::Default, weight, italic, None, None)
                .unwrap(),
        );
        let typography = Typography::new(&theme, None, None, 0, Arc::from([]));
        let (index, mut metrics) = wrap("abc", 35., typography);
        assert_eq!(metrics.width_at("a", 0), px(advance));
        assert_eq!(index.row_count(), if advance == 10. { 1 } else { 2 });
    }
    let typography = Typography::new(
        &ThemeSettings::default(),
        None,
        None,
        0,
        Arc::from([FontSpan {
            range: 0..2,
            bold: Some(true),
            italic: Some(true),
        }]),
    );
    let (index, mut metrics) = wrap("AV\tb", 85., typography);
    assert_eq!(index.row(0).unwrap().row.content_len, 3);
    assert_eq!(index.row(1).unwrap().row.tab_phase, px(0.));
    assert_eq!(metrics.width_at("A", 0) + metrics.width_at("V", 1), px(32.));
    assert_eq!(metrics.tab_width(px(32.)), px(48.));
}

#[test]
fn font_emphasis_inherits_and_explicit_normal_can_cancel_the_base_font() {
    let base = font("monospace").bold().italic();
    assert_eq!(FontEmphasis::default().font(&base), base);
    let normal = FontEmphasis {
        bold: Some(false),
        italic: Some(false),
    }
    .font(&base);
    assert_eq!(normal.weight, FontWeight::NORMAL);
    assert_eq!(normal.style, FontStyle::Normal);
}

#[gpui_kit::test]
fn font_runs_and_wraps_preserve_graphemes_and_use_matching_styles(cx: &mut TestAppContext) {
    let text = "AV ffi e\u{301}👩‍💻\t中🙂 abcdef\n".repeat(40);
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.clone(),
            format: FileFormat::default(),
        },
    );
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let mut theme = ThemeSettings::default();
    theme.set_style(
        StyleRole::Default,
        StyleOverride::default()
            .with_text_attributes(StyleRole::Default, Some("bold"), Some(true), None, None)
            .unwrap(),
    );
    let mut settings = settings();
    settings.typography = Typography::new(
        &theme,
        None,
        None,
        0,
        Arc::from([
            FontSpan {
                range: 10..12,
                bold: Some(false),
                italic: None,
            },
            FontSpan {
                range: 20..23,
                bold: None,
                italic: Some(false),
            },
        ]),
    );
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    let index = WrapJob::new(snapshot, settings.clone(), None, None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    for row in index.visible_rows(0..index.row_count()) {
        let content: String = index
            .text()
            .chunks_in_range(row.start.0..row.start.0 + row.row.content_len)
            .collect();
        let mut phase = row.row.tab_phase;
        let mut width = px(0.);
        for (offset, grapheme) in content.grapheme_indices(true) {
            let advance = if grapheme == "\t" {
                metrics.tab_width(phase)
            } else {
                metrics.width_at(grapheme, row.start.0 + offset)
            };
            width += advance;
            phase = metrics.advance_phase(phase, advance);
        }
        assert!(
            width <= settings.wrap_width || content.graphemes(true).count() <= 1,
            "{content:?}: {width:?}"
        );
        let no_tabs = content.split('\t').next().unwrap();
        let boundaries: Vec<_> = no_tabs
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain([no_tabs.len()])
            .collect();
        let mut position = 0;
        for run in metrics.runs(no_tabs, row.start.0) {
            position += run.len;
            assert!(boundaries.contains(&position));
        }
        assert_eq!(position, no_tabs.len());
    }
}

#[gpui_kit::test]
fn grapheme_measurement_caches_remain_bounded(cx: &mut TestAppContext) {
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings());
    for ch in (0x4e00..0x4e00 + 5000).filter_map(char::from_u32) {
        metrics.width(&ch.to_string());
        assert!(metrics.measurements <= 256);
        assert!(metrics.other_entries <= 4096);
    }
    assert_eq!(metrics.other_entries, 4096);
}

#[test]
fn different_document_lines_do_not_trigger_full_line_backend_shaping() {
    use crate::test_support::StyledTextSystem;
    for lines in [100, 10_000] {
        let backend = Arc::new(StyledTextSystem::default());
        let system = Arc::new(TextSystem::new(backend.clone()));
        let text: String = (0..lines)
            .map(|i| format!("AV fi e\u{301}👩‍💻 中 line_{i}\tend\n"))
            .collect();
        let buffer = Buffer::new(
            None,
            DecodedFile {
                text,
                format: FileFormat::default(),
            },
        );
        let snapshot = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let mut settings = settings();
        settings.typography = Typography::new(
            &ThemeSettings::default(),
            None,
            None,
            0,
            Arc::from([FontSpan {
                range: 0..buffer.text().len(),
                bold: Some(true),
                italic: Some(true),
            }]),
        );
        let mut metrics = FontMeasure::new(system, settings.clone());
        let index = WrapJob::new(snapshot, settings, None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        assert!(index.is_complete());
        assert_eq!(index.indexed_bytes(), buffer.text().len());
        assert_eq!(backend.contextual_layouts.load(AtomicOrdering::Relaxed), 0);
        assert!(backend.layouts.load(AtomicOrdering::Relaxed) <= 32);
    }
}

#[gpui_kit::test]
fn markdown_nested_emphasis_and_headings_compose_without_leaking_into_code(
    cx: &mut TestAppContext,
) {
    let text = "# Heading **strong** *italic* `code`\n\n***both***\n\n**outer *inner* outer**\n\n```rust\nfn main() {}\n```\n";
    let buffer = Buffer::new(
        Some("styles.md".into()),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    );
    let observer = cx.new(|_| ());
    cx.update(|cx| buffer.prepare_syntax(observer.downgrade().into(), cx));
    cx.run_until_parked();
    let theme = ThemeSettings::default();
    let typography = Typography::new(&theme, buffer.syntax_display(), None, 0, Arc::from([]));
    for token in ["Heading", "strong", "italic", "code", "both"] {
        assert_eq!(
            typography.font_at(text.find(token).unwrap()).bold,
            Some(true),
            "{token}"
        );
    }
    for token in ["italic", "both"] {
        assert_eq!(
            typography.font_at(text.find(token).unwrap()).italic,
            Some(true),
            "{token}"
        );
    }
    assert_eq!(
        typography.font_at(text.find("fn main").unwrap()),
        FontEmphasis::default()
    );
    let mut theme = theme;
    theme.set_style(
        StyleRole::MarkupEmphasis,
        StyleOverride::default()
            .with_text_attributes(
                StyleRole::MarkupEmphasis,
                Some("normal"),
                Some(false),
                None,
                None,
            )
            .unwrap(),
    );
    let typography = Typography::new(&theme, buffer.syntax_display(), None, 0, Arc::from([]));
    let inner = typography.font_at(text.find("inner").unwrap());
    assert_eq!(inner.bold, Some(false));
    assert_eq!(inner.italic, Some(false));
}

#[test]
fn marked_text_uses_base_font_and_projects_later_external_styles() {
    let typography = Typography::new(
        &ThemeSettings::default(),
        None,
        Some(1..4),
        0,
        Arc::from([FontSpan {
            range: 0..4,
            bold: Some(true),
            italic: None,
        }]),
    );
    assert_eq!(typography.font_at(0).bold, Some(true));
    assert_eq!(typography.font_at(1).bold, None);
    assert_eq!(typography.font_at(3).bold, None);
    assert_eq!(typography.font_at(4).bold, Some(true));
    assert_eq!(typography.font_at(6).bold, Some(true));
}
