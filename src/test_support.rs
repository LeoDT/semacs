use gpui_kit::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, FontStyle, FontWeight, GlyphId,
    LineLayout, NoopTextSystem, Pixels, PlatformTextSystem, RenderGlyphParams, Result, ShapedGlyph,
    ShapedRun, Size, TestAppContext, TestDispatcher, TextRenderingMode, point, px, size,
};
use std::borrow::Cow;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(crate) struct StyledTextSystem {
    pub layouts: AtomicUsize,
    pub contextual_layouts: AtomicUsize,
}

impl StyledTextSystem {
    fn advance_width(font_id: FontId) -> f32 {
        let flags = font_id.0.saturating_sub(1);
        10. + if flags & 1 != 0 { 4. } else { 0. } + if flags & 2 != 0 { 2. } else { 0. }
    }
}

impl PlatformTextSystem for StyledTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        NoopTextSystem.add_fonts(fonts)
    }
    fn all_font_names(&self) -> Vec<String> {
        vec!["monospace".into()]
    }
    fn font_id(&self, font: &Font) -> Result<FontId> {
        Ok(FontId(
            1 + usize::from(font.weight >= FontWeight::BOLD)
                + 2 * usize::from(font.style == FontStyle::Italic),
        ))
    }
    fn font_metrics(&self, id: FontId) -> FontMetrics {
        NoopTextSystem.font_metrics(id)
    }
    fn typographic_bounds(&self, font: FontId, glyph: GlyphId) -> Result<Bounds<f32>> {
        NoopTextSystem.typographic_bounds(font, glyph)
    }
    fn advance(&self, font: FontId, glyph: GlyphId) -> Result<Size<f32>> {
        Ok(size(
            Self::advance_width(font) * 1000. / 16. * glyph.0 as f32,
            0.,
        ))
    }
    fn glyph_for_char(&self, font: FontId, ch: char) -> Option<GlyphId> {
        NoopTextSystem.glyph_for_char(font, ch)
    }
    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        NoopTextSystem.glyph_raster_bounds(params)
    }
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        NoopTextSystem.rasterize_glyph(params, bounds)
    }
    fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }

    fn layout_line(&self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        self.layouts.fetch_add(1, Ordering::Relaxed);
        if text.graphemes(true).count() > 1 {
            self.contextual_layouts.fetch_add(1, Ordering::Relaxed);
        }
        let mut position = px(0.);
        let mut runs: Vec<ShapedRun> = Vec::new();
        let ends: Vec<_> = font_runs
            .iter()
            .scan(0, |end, run| {
                *end += run.len;
                Some((*end, run.font_id))
            })
            .collect();
        let font_at = |offset| {
            ends.iter()
                .find(|(end, _)| offset < *end)
                .map_or(FontId(1), |(_, id)| *id)
        };
        let mut graphemes = text.grapheme_indices(true).peekable();
        let mut previous = None;
        while let Some((offset, grapheme)) = graphemes.next() {
            let font_id = font_at(offset);
            let unit = font_size / 16. * Self::advance_width(font_id);
            let glyph = NoopTextSystem
                .glyph_for_char(font_id, grapheme.chars().next().unwrap())
                .unwrap();
            let ligature = grapheme == "f"
                && graphemes
                    .peek()
                    .is_some_and(|(next, text)| *text == "i" && font_at(*next) == font_id);
            if ligature {
                graphemes.next();
            }
            let shaped = ShapedGlyph {
                id: glyph,
                position: point(position, px(0.)),
                index: offset,
                is_emoji: glyph.0 == 2,
            };
            if let Some(run) = runs.last_mut().filter(|run| run.font_id == font_id) {
                run.glyphs.push(shaped);
            } else {
                runs.push(ShapedRun {
                    font_id,
                    glyphs: vec![shaped],
                });
            }
            // Both contraction and expansion expose disagreement with isolated grapheme sums.
            position += if ligature || (grapheme == "V" && previous == Some(("A", font_id))) {
                unit * 1.5
            } else {
                unit * glyph.0 as f32
            };
            previous = Some((grapheme, font_id));
        }
        LineLayout {
            font_size,
            width: position,
            ascent: font_size,
            descent: font_size / 4.,
            runs,
            len: text.len(),
        }
    }
}

pub(crate) fn with_styled_text_system(run: impl FnOnce(&mut TestAppContext)) {
    with_text_system(Arc::new(StyledTextSystem::default()), run);
}

pub(crate) fn with_text_system(
    system: Arc<dyn PlatformTextSystem>,
    run: impl FnOnce(&mut TestAppContext),
) {
    let dispatcher = TestDispatcher::new(0);
    let mut cx =
        TestAppContext::build_with_text_system(dispatcher.clone(), Some("font_geometry"), system);
    run(&mut cx);
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
    drop(cx);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
}
