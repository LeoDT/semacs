use super::{RowPart, cursor::Cursor};
use crate::syntax::DisplayHighlights;
use crate::theme::{StyleRole, ThemeSettings};
use gpui_kit::{App, DecorationRun, Hsla, LineLayout, Pixels, Point, TextAlign, Window};
use smallvec::SmallVec;
use std::ops::Range;

pub(super) struct TextStyle<'a> {
    colors: [Hsla; StyleRole::COUNT],
    pub syntax: Option<&'a DisplayHighlights>,
    pub marked: Option<Range<usize>>,
    pub prompt_end: usize,
}

impl<'a> TextStyle<'a> {
    pub(super) fn new(
        theme: &ThemeSettings,
        syntax: Option<&'a DisplayHighlights>,
        marked: Option<Range<usize>>,
        prompt_end: usize,
    ) -> Self {
        Self {
            colors: StyleRole::ALL.map(|role| theme.style(role).foreground),
            syntax,
            marked,
            prompt_end,
        }
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
            let mut role = StyleRole::Default;
            if let Some(span) = syntax.peek() {
                if span.range.start <= position {
                    role = span.style;
                    end = end.min(span.range.end);
                } else {
                    end = end.min(span.range.start);
                }
            }
            if position < self.prompt_end {
                role = StyleRole::MinibufferPrompt;
                end = end.min(self.prompt_end);
            }
            if let Some(cursor) = &cursor {
                if position < cursor.start {
                    end = end.min(cursor.start);
                } else if position < cursor.end {
                    role = StyleRole::Cursor;
                    end = end.min(cursor.end);
                }
            }
            let color = self.colors[role as usize];
            let len = (end - position) as u32;
            if let Some(last) = runs.last_mut()
                && last.color == color
            {
                last.len += len;
            } else {
                runs.push(DecorationRun {
                    len,
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            position = end;
        }
        runs
    }
}

impl RowPart {
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
        let runs = style.runs(self.range.start.0..self.range.end.0, cursor);
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
