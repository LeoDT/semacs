use super::*;
use crate::theme::FontEmphasis;
use gpui_kit::{FontStyle, FontWeight, PlatformTextSystem, TextRun, WindowTextSystem, black};
use gpui_wgpu::CosmicTextSystem;
use std::sync::Arc;

#[test]
fn startup_loads_all_default_font_faces_without_system_fonts() {
    let backend = Arc::new(CosmicTextSystem::new_without_system_fonts("Lilex"));
    assert!(backend.all_font_names().is_empty());
    crate::test_support::with_text_system(backend.clone(), |cx| {
        cx.update(crate::app::init);
        cx.update(|cx| {
            let settings = cx.default_global::<FontSettings>().clone();
            assert_eq!(settings.family(), "Lilex");
            assert!(
                backend
                    .all_font_names()
                    .iter()
                    .any(|name| name == settings.family())
            );
            let mut ids = Vec::new();
            let runs: Vec<_> = [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .map(|(bold, italic)| {
                    let font = FontEmphasis {
                        bold: Some(bold),
                        italic: Some(italic),
                    }
                    .font(&settings.font());
                    let id = backend.font_id(&font).unwrap();
                    assert_eq!(cx.text_system().resolve_font(&font), id);
                    assert!(!ids.contains(&id));
                    ids.push(id);
                    assert_eq!(
                        cx.text_system().advance(id, settings.size(), 'i').unwrap(),
                        cx.text_system().advance(id, settings.size(), 'W').unwrap(),
                    );
                    assert_eq!(
                        backend.font_weight_and_style(id).unwrap(),
                        (
                            if bold {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            },
                            if italic {
                                FontStyle::Italic
                            } else {
                                FontStyle::Normal
                            },
                        )
                    );
                    TextRun {
                        len: 1,
                        font,
                        color: black(),
                        ..Default::default()
                    }
                })
                .collect();
            let system = WindowTextSystem::new(cx.text_system().clone());
            let line = system.shape_line("abcd".into(), settings.size(), &runs, None);
            assert_eq!(
                line.runs.iter().map(|run| run.glyphs.len()).sum::<usize>(),
                4
            );
            for run in &line.runs {
                for glyph in &run.glyphs {
                    assert_eq!(run.font_id, ids[glyph.index]);
                }
            }
            assert!(line.width > px(0.));
        });
    });
}
