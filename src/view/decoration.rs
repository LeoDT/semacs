use super::{RowPart, ViewportLayout, cursor::Cursor};
use crate::syntax::DisplayHighlights;
use crate::theme::{StyleOverride, StyleRole, ThemeSettings};
use gpui_kit::{
    App, Bounds, DecorationRun, LineLayout, Pixels, Point, StrikethroughStyle, TextAlign,
    UnderlineStyle, Window, fill, point, px, size,
};
use smallvec::SmallVec;
use std::ops::Range;

pub(super) struct TextStyle<'a> {
    styles: [StyleOverride; StyleRole::COUNT],
    patches: [StyleOverride; StyleRole::COUNT],
    pub syntax: Option<&'a DisplayHighlights>,
    pub marked: Option<Range<usize>>,
    pub prompt_end: usize,
    annotations: Option<&'a crate::buffer::Annotations>,
    named: &'a [(Range<crate::text::ByteOffset>, StyleRole)],
    current_section: Option<Range<crate::text::ByteOffset>>,
}

impl<'a> TextStyle<'a> {
    pub(super) fn new(
        theme: &ThemeSettings,
        syntax: Option<&'a DisplayHighlights>,
        marked: Option<Range<usize>>,
        prompt_end: usize,
    ) -> Self {
        Self {
            styles: StyleRole::ALL.map(|role| theme.attributes(role)),
            patches: StyleRole::ALL.map(|role| theme.patch(role)),
            syntax,
            marked,
            prompt_end,
            annotations: None,
            named: &[],
            current_section: None,
        }
    }

    pub(super) fn with_annotations(
        mut self,
        annotations: Option<&'a crate::buffer::Annotations>,
    ) -> Self {
        self.annotations = annotations;
        self
    }

    pub(super) fn with_named_ranges(
        mut self,
        named: &'a [(Range<crate::text::ByteOffset>, StyleRole)],
    ) -> Self {
        self.named = named;
        self
    }

    pub(super) fn with_current_section(
        mut self,
        current: Option<Range<crate::text::ByteOffset>>,
    ) -> Self {
        self.current_section = current;
        self
    }

    fn named_role(&self, role: StyleRole, offset: crate::text::ByteOffset) -> StyleRole {
        if self
            .current_section
            .as_ref()
            .is_some_and(|range| range.contains(&offset))
        {
            role.highlighted()
        } else {
            role
        }
    }

    pub(super) fn named_line_background(
        &self,
        offset: crate::text::ByteOffset,
    ) -> Option<gpui_kit::Hsla> {
        let index = self.named.partition_point(|(range, _)| range.end <= offset);
        let background = self
            .named
            .get(index)
            .filter(|(range, _)| range.contains(&offset))
            .and_then(|(_, role)| self.styles[self.named_role(*role, offset) as usize].background);
        background.or_else(|| {
            self.current_section
                .as_ref()
                .filter(|range| range.contains(&offset))
                .map(|_| {
                    self.styles[StyleRole::SectionCurrent as usize]
                        .background
                        .unwrap()
                })
        })
    }

    fn runs(
        &self,
        range: Range<usize>,
        cursor: Option<Range<usize>>,
    ) -> SmallVec<[DecorationRun; 4]> {
        let mut syntax = self
            .syntax
            .into_iter()
            .flat_map(|syntax| syntax.in_display_range(range.clone(), self.marked.clone()))
            .peekable();
        let mut runs: SmallVec<[DecorationRun; 4]> = SmallVec::new();
        let mut position = range.start;
        while position < range.end {
            while syntax.peek().is_some_and(|span| span.range.end <= position) {
                syntax.next();
            }
            let mut end = range.end;
            let mut attributes = self.styles[StyleRole::Default as usize];
            attributes.background = None;
            if let Some(span) = syntax.peek() {
                if span.range.start <= position {
                    if span.style != StyleRole::Default {
                        attributes.merge(self.styles[span.style as usize]);
                    }
                    for role in &span.modifiers {
                        attributes.merge(self.patches[*role as usize]);
                    }
                    end = end.min(span.range.end);
                } else {
                    end = end.min(span.range.start);
                }
            }
            let committed = match &self.marked {
                Some(marked) if position < marked.start => {
                    end = end.min(marked.start);
                    Some(position)
                }
                Some(marked) if position < marked.end => {
                    end = end.min(marked.end);
                    None
                }
                Some(marked) => Some(position - marked.len()),
                None => Some(position),
            };
            if let (Some(annotations), Some(committed)) = (self.annotations, committed) {
                let delta = position - committed;
                let index = annotations
                    .ranges
                    .partition_point(|span| span.range.end.0 <= committed);
                if let Some(span) = annotations.ranges.get(index) {
                    if span.range.start.0 <= committed {
                        attributes.merge(span.style);
                        end = end.min(span.range.end.0 + delta);
                    } else {
                        end = end.min(span.range.start.0 + delta);
                    }
                }
            }
            let index = self
                .named
                .partition_point(|(range, _)| range.end.0 <= position);
            if let Some((range, role)) = self.named.get(index) {
                if range.start.0 <= position {
                    let role = self.named_role(*role, crate::text::ByteOffset(position));
                    attributes.merge(self.styles[role as usize]);
                    end = end.min(range.end.0);
                    if let Some(current) = &self.current_section {
                        if position < current.start.0 {
                            end = end.min(current.start.0);
                        } else if position < current.end.0 {
                            end = end.min(current.end.0);
                        }
                    }
                } else {
                    end = end.min(range.start.0);
                }
            }
            if position < self.prompt_end {
                attributes = self.styles[StyleRole::Default as usize];
                attributes.background = None;
                attributes.merge(self.styles[StyleRole::MinibufferPrompt as usize]);
                end = end.min(self.prompt_end);
            }
            if let Some(cursor) = &cursor {
                if position < cursor.start {
                    end = end.min(cursor.start);
                } else if position < cursor.end {
                    attributes.foreground = self.styles[StyleRole::Cursor as usize].foreground;
                    end = end.min(cursor.end);
                }
            }
            let color = attributes.foreground.expect("resolved foreground");
            let run = DecorationRun {
                len: (end - position) as u32,
                color,
                background_color: attributes.background,
                underline: attributes
                    .underline
                    .unwrap_or(false)
                    .then_some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    }),
                strikethrough: attributes.strikethrough.unwrap_or(false).then_some(
                    StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    },
                ),
            };
            if let Some(last) = runs.last_mut()
                && last.color == run.color
                && last.background_color == run.background_color
                && last.underline == run.underline
                && last.strikethrough == run.strikethrough
            {
                last.len += run.len;
            } else {
                runs.push(run);
            }
            position = end;
        }
        runs
    }
}

impl ViewportLayout {
    pub(super) fn line_backgrounds(
        &self,
        annotations: &crate::buffer::Annotations,
        committed_text: &crate::text::Rope,
        marked: Option<Range<usize>>,
    ) -> Vec<(Bounds<Pixels>, gpui_kit::Hsla)> {
        self.rows
            .iter()
            .filter_map(|row| {
                let offset = row.source.start.0;
                let committed = match &marked {
                    Some(marked) if offset >= marked.end => offset - marked.len(),
                    Some(marked) if offset >= marked.start => marked.start,
                    _ => offset,
                };
                let logical_row = committed_text
                    .offset_to_point(committed.min(committed_text.len()))
                    .row;
                let color = annotations.line_color(logical_row)?;
                Some((
                    Bounds::new(
                        point(self.bounds.origin.x, self.origin(row).y),
                        size(self.bounds.size.width, self.key.settings.line_height),
                    ),
                    color,
                ))
            })
            .collect()
    }
}

impl RowPart {
    pub(super) fn paint_background(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        style: &TextStyle<'_>,
        window: &mut Window,
    ) {
        let mut offset = 0;
        for run in style.runs(self.range.start.0..self.range.end.0, None) {
            let end = offset + run.len as usize;
            if let Some(color) = run.background_color {
                let left = self
                    .line
                    .as_ref()
                    .map_or(px(0.), |line| line.x_for_index(offset));
                let right = self
                    .line
                    .as_ref()
                    .map_or(self.width, |line| line.x_for_index(end));
                window.paint_quad(fill(
                    Bounds::new(
                        point(origin.x + left, origin.y),
                        size((right - left).max(px(0.)), line_height),
                    ),
                    color,
                ));
            }
            offset = end;
        }
    }

    pub(super) fn paint_text(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        cursor: Option<&Cursor>,
        style: &TextStyle<'_>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(line) = &self.line else {
            return;
        };
        let cursor = cursor.map(|cursor| cursor.range.start.0..cursor.range.end.0);
        let mut runs = style.runs(self.range.start.0..self.range.end.0, cursor);
        for run in &mut runs {
            run.background_color = None;
        }
        LineLayout::paint(
            line,
            origin,
            line_height,
            TextAlign::Left,
            None,
            &runs,
            window,
            cx,
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::StyleOverride;

    #[gpui_kit::test]
    fn range_backgrounds_preserve_syntax_decorations_and_cursor_foreground(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::buffer::{Buffer, StyledRange};
        use crate::files::{DecodedFile, FileFormat};
        use crate::text::ByteOffset;
        use gpui_kit::AppContext;
        let mut buffer = Buffer::new(
            Some("style.rs".into()),
            DecodedFile {
                text: "fn main() {}".into(),
                format: FileFormat::default(),
            },
        );
        let observer = cx.new(|_| ());
        cx.update(|cx| buffer.prepare_syntax(observer.downgrade().into(), cx));
        cx.run_until_parked();
        buffer
            .set_annotations(
                buffer.snapshot_id(),
                vec![StyledRange {
                    range: ByteOffset(0)..ByteOffset(1),
                    style: StyleOverride::new(StyleRole::Default, None, Some("#223344")).unwrap(),
                }],
                vec![],
            )
            .unwrap();
        let mut theme = ThemeSettings::default();
        theme.set_style(
            StyleRole::SyntaxKeyword,
            StyleOverride::new(StyleRole::SyntaxKeyword, None, Some("#112233"))
                .unwrap()
                .with_text_attributes(
                    StyleRole::SyntaxKeyword,
                    Some("bold"),
                    None,
                    Some(true),
                    Some(true),
                )
                .unwrap(),
        );
        let syntax = buffer.syntax_display().unwrap();
        let annotations = buffer.annotations().unwrap();
        let style =
            TextStyle::new(&theme, Some(&syntax), None, 0).with_annotations(Some(&annotations));
        let runs = style.runs(0..3, Some(0..1));
        assert_eq!(runs[0].color, theme.style(StyleRole::Cursor).foreground);
        assert_eq!(
            runs[0].background_color,
            Some(gpui_kit::rgb(0x223344).into())
        );
        assert!(runs[0].underline.is_some());
        assert!(runs[0].strikethrough.is_some());
        assert_eq!(
            runs[1].background_color,
            Some(gpui_kit::rgb(0x112233).into())
        );
        assert_eq!(runs.last().unwrap().background_color, None);
        let style = TextStyle::new(&theme, Some(&syntax), Some(1..4), 0)
            .with_annotations(Some(&annotations));
        let runs = style.runs(1..4, None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].color, theme.style(StyleRole::Default).foreground);
        assert!(runs[0].background_color.is_none());
        assert!(runs[0].underline.is_none());
    }

    #[test]
    fn named_line_backgrounds_and_text_share_current_section_variants() {
        use crate::text::ByteOffset as B;
        let mut theme = ThemeSettings::default();
        theme.set_style(
            StyleRole::DiffAddedHighlight,
            StyleOverride::new(
                StyleRole::DiffAddedHighlight,
                Some("#123456"),
                Some("#234567"),
            )
            .unwrap(),
        );
        let named = [
            (B(0)..B(2), StyleRole::DiffAdded),
            (B(2)..B(4), StyleRole::DiffContext),
        ];
        let plain = TextStyle::new(&theme, None, None, 0).with_named_ranges(&named);
        let current = TextStyle::new(&theme, None, None, 0)
            .with_named_ranges(&named)
            .with_current_section(Some(B(0)..B(4)));
        assert_eq!(
            plain.named_line_background(B(0)),
            Some(theme.style(StyleRole::DiffAdded).background)
        );
        assert_eq!(plain.named_line_background(B(2)), None);
        assert_eq!(
            current.named_line_background(B(0)),
            Some(theme.style(StyleRole::DiffAddedHighlight).background)
        );
        assert_eq!(
            current.named_line_background(B(2)),
            Some(theme.style(StyleRole::SectionCurrent).background)
        );
        assert_eq!(current.named_line_background(B(4)), None);
        let runs = current.runs(0..4, Some(0..1));
        assert_eq!(runs[0].color, theme.style(StyleRole::Cursor).foreground);
        assert_eq!(
            runs[1].color,
            theme.style(StyleRole::DiffAddedHighlight).foreground
        );
        assert_eq!(
            runs[1].background_color,
            current.named_line_background(B(0))
        );
        let partial = TextStyle::new(&theme, None, None, 0)
            .with_named_ranges(&named)
            .with_current_section(Some(B(1)..B(2)));
        let runs = partial.runs(0..2, None);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].color, theme.style(StyleRole::DiffAdded).foreground);
        assert_eq!(
            runs[1].color,
            theme.style(StyleRole::DiffAddedHighlight).foreground
        );
    }

    #[test]
    fn prompt_and_cursor_preserve_plain_body_colors() {
        let mut theme = ThemeSettings::default();
        for (role, color) in [
            (StyleRole::Cursor, "#00ff00"),
            (StyleRole::MinibufferPrompt, "#ff0000"),
        ] {
            theme.set_style(role, StyleOverride::new(role, Some(color), None).unwrap());
        }
        for cursor in [None, Some(0..1), Some(2..5), Some(4..7)] {
            let style = TextStyle::new(&theme, None, None, 4);
            let runs = style.runs(0..8, cursor.clone());
            let mut offset = 0;
            for run in &runs {
                for byte in offset..offset + run.len as usize {
                    let role = if cursor.as_ref().is_some_and(|range| range.contains(&byte)) {
                        StyleRole::Cursor
                    } else if byte < 4 {
                        StyleRole::MinibufferPrompt
                    } else {
                        StyleRole::Default
                    };
                    assert_eq!(run.color, theme.style(role).foreground);
                }
                offset += run.len as usize;
            }
            assert_eq!(offset, 8);
            assert!(!runs.spilled());
        }
    }

    #[gpui_kit::test]
    fn syntax_runs_preserve_preedit_prompt_and_cursor_priority_without_truncation(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::buffer::Buffer;
        use crate::files::{DecodedFile, FileFormat};
        use gpui_kit::AppContext;
        let text = "fn main() { let x = 42; } ".repeat(200);
        let buffer = Buffer::new(
            Some("decorations.rs".into()),
            DecodedFile {
                text: text.clone(),
                format: FileFormat::default(),
            },
        );
        let observer = cx.new(|_| ());
        cx.update(|cx| buffer.prepare_syntax(observer.downgrade().into(), cx));
        cx.run_until_parked();
        let syntax = buffer.syntax_display().unwrap();
        let theme = ThemeSettings::default();
        let marked = 1..4;
        let cursor = 3..7;
        let style = TextStyle::new(&theme, Some(&syntax), Some(marked.clone()), 1);
        let runs = style.runs(0..text.len() + marked.len(), Some(cursor.clone()));
        assert!(runs.spilled());
        let mut offset = 0;
        for run in runs {
            assert!(run.background_color.is_none());
            for byte in offset..offset + run.len as usize {
                let role = if cursor.contains(&byte) {
                    StyleRole::Cursor
                } else if byte < 1 {
                    StyleRole::MinibufferPrompt
                } else if marked.contains(&byte) {
                    StyleRole::Default
                } else {
                    let committed = byte - marked.len();
                    syntax
                        .in_range(committed..committed + 1)
                        .next()
                        .map_or(StyleRole::Default, |span| span.style)
                };
                assert_eq!(run.color, theme.style(role).foreground, "byte {byte}");
            }
            offset += run.len as usize;
        }
        assert_eq!(offset, text.len() + marked.len());
    }

    #[test]
    fn cursor_decorations_invert_only_selected_bytes() {
        let theme = ThemeSettings::default();
        let style = TextStyle::new(&theme, None, None, 0);
        for range in [0..1, 1..4, 4..5, 0..5] {
            let runs = style.runs(0..5, Some(range.clone()));
            let mut offset = 0;
            for run in &runs {
                let end = offset + run.len as usize;
                assert!(
                    end <= range.start
                        || offset >= range.end
                        || (offset == range.start && end == range.end)
                );
                assert_eq!(
                    run.color,
                    theme
                        .style(if offset == range.start {
                            StyleRole::Cursor
                        } else {
                            StyleRole::Default
                        })
                        .foreground
                );
                offset = end;
            }
            assert_eq!(offset, 5);
        }
    }
}
