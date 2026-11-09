use std::ops::Range;
use std::sync::Arc;

use gpui_kit::TextRun;
use unicode_segmentation::UnicodeSegmentation;

use crate::syntax::DisplayHighlightSnapshot;
use crate::theme::{FontEmphasis, StyleRole, ThemeSettings};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSpan {
    pub range: Range<usize>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SectionFonts {
    range: Range<crate::text::ByteOffset>,
    roles: [FontEmphasis; StyleRole::COUNT],
    changed: bool,
}

#[derive(Clone)]
pub struct Typography {
    base: FontEmphasis,
    prompt: FontEmphasis,
    prompt_end: usize,
    syntax: Option<DisplayHighlightSnapshot>,
    marked: Option<Range<usize>>,
    roles: [(Option<bool>, Option<bool>); StyleRole::COUNT],
    overlays: Arc<[FontSpan]>,
    named: Arc<[(Range<crate::text::ByteOffset>, StyleRole)]>,
    named_roles: [FontEmphasis; StyleRole::COUNT],
    section_fonts: Option<Arc<SectionFonts>>,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            base: FontEmphasis::default(),
            prompt: FontEmphasis::default(),
            prompt_end: 0,
            syntax: None,
            marked: None,
            roles: [(None, None); StyleRole::COUNT],
            overlays: Arc::from([]),
            named: Arc::from([]),
            named_roles: [FontEmphasis::default(); StyleRole::COUNT],
            section_fonts: None,
        }
    }
}

impl std::fmt::Debug for Typography {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Typography")
            .field("base", &self.base)
            .field("prompt_end", &self.prompt_end)
            .field("styled_syntax", &self.syntax.is_some())
            .field("overlays", &self.overlays)
            .finish()
    }
}

impl PartialEq for Typography {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base
            && self.prompt == other.prompt
            && self.prompt_end == other.prompt_end
            && self.roles == other.roles
            && self.named_roles == other.named_roles
            && self.active_section_fonts() == other.active_section_fonts()
            && (Arc::ptr_eq(&self.named, &other.named) || self.named == other.named)
            && self.marked == other.marked
            && match (&self.syntax, &other.syntax) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
            && (Arc::ptr_eq(&self.overlays, &other.overlays) || self.overlays == other.overlays)
    }
}

impl Typography {
    pub(crate) fn new(
        theme: &ThemeSettings,
        syntax: Option<DisplayHighlightSnapshot>,
        marked: Option<Range<usize>>,
        prompt_end: usize,
        overlays: Arc<[FontSpan]>,
    ) -> Self {
        let base = theme
            .patch(StyleRole::Default)
            .emphasis(FontEmphasis::default());
        let prompt = theme.patch(StyleRole::MinibufferPrompt).emphasis(base);
        let mask = syntax.as_ref().map_or(0, |syntax| syntax.role_mask());
        let roles = StyleRole::ALL.map(|role| {
            let patch = theme.patch(role);
            if mask.checked_shr(role as u32).unwrap_or(0) & 1 != 0 {
                (patch.bold, patch.italic)
            } else {
                (None, None)
            }
        });
        let syntax = syntax.filter(|_| {
            roles.iter().any(|(bold, italic)| {
                bold.is_some_and(|bold| Some(bold) != base.bold)
                    || italic.is_some_and(|italic| Some(italic) != base.italic)
            })
        });
        let roles = if syntax.is_some() {
            roles
        } else {
            [(None, None); StyleRole::COUNT]
        };
        let marked = if syntax.is_some() || !overlays.is_empty() {
            marked
        } else {
            None
        };
        Self {
            base,
            prompt,
            prompt_end,
            marked,
            syntax,
            roles,
            overlays,
            named: Arc::from([]),
            named_roles: [FontEmphasis::default(); StyleRole::COUNT],
            section_fonts: None,
        }
    }

    pub(crate) fn with_named_ranges(
        mut self,
        named: Arc<[(Range<crate::text::ByteOffset>, StyleRole)]>,
        theme: &ThemeSettings,
    ) -> Self {
        if !named.is_empty() {
            self.named_roles =
                StyleRole::ALL.map(|role| theme.attributes(role).emphasis(FontEmphasis::default()));
            self.named = named;
        }
        self
    }

    fn active_section_fonts(&self) -> Option<&SectionFonts> {
        self.section_fonts.as_deref().filter(|fonts| fonts.changed)
    }

    pub(crate) fn with_current_section(
        mut self,
        current: Option<Range<crate::text::ByteOffset>>,
        theme: &ThemeSettings,
        previous: Option<&Self>,
    ) -> Self {
        let Some(current) = current else { return self };
        let highlighted = StyleRole::ALL.map(|role| {
            theme
                .attributes(role.highlighted())
                .emphasis(FontEmphasis::default())
        });
        if let Some(previous) = previous.filter(|previous| {
            Arc::ptr_eq(&self.named, &previous.named)
                && self.base == previous.base
                && self.named_roles == previous.named_roles
        }) && let Some(fonts) = previous
            .section_fonts
            .as_ref()
            .filter(|fonts| fonts.range == current && fonts.roles == highlighted)
        {
            self.section_fonts = Some(fonts.clone());
            return self;
        }
        let start = self
            .named
            .partition_point(|(range, _)| range.end <= current.start);
        let changed = highlighted != self.named_roles
            && self.named[start..]
                .iter()
                .take_while(|(range, _)| range.start < current.end)
                .any(|(_, role)| {
                    highlighted[*role as usize].emphasis(self.base)
                        != self.named_roles[*role as usize].emphasis(self.base)
                });
        self.section_fonts = Some(Arc::new(SectionFonts {
            range: current,
            roles: highlighted,
            changed,
        }));
        self
    }

    pub(crate) fn compatible_after_edit(&self, other: &Self) -> bool {
        self.base == other.base
            && self.prompt == other.prompt
            && self.prompt_end == other.prompt_end
            && self.roles == other.roles
            && self.overlays.is_empty()
            && other.overlays.is_empty()
            && self.named.is_empty()
            && other.named.is_empty()
            && match (&self.syntax, &other.syntax) {
                (None, None) => true,
                (Some(a), Some(b)) => a.shares_source(b),
                _ => false,
            }
    }

    pub(crate) fn base(&self) -> FontEmphasis {
        self.base
    }

    pub(crate) fn is_uniform(&self) -> bool {
        self.syntax.is_none()
            && self.overlays.is_empty()
            && self.named.is_empty()
            && (self.prompt_end == 0 || self.prompt == self.base)
    }

    pub(crate) fn font_at(&self, offset: usize) -> FontEmphasis {
        if self.is_uniform() {
            return self.base;
        }
        if offset < self.prompt_end {
            return self.prompt;
        }
        let mut emphasis = self.base;
        if let Some(syntax) = &self.syntax
            && let Some(span) = syntax
                .in_display_range(offset..offset + 1, self.marked.clone())
                .next()
        {
            for role in std::iter::once(span.style).chain(span.modifiers.iter().copied()) {
                let (bold, italic) = self.roles[role as usize];
                emphasis = FontEmphasis {
                    bold: bold.or(emphasis.bold),
                    italic: italic.or(emphasis.italic),
                };
            }
        }
        let committed = if let Some(marked) = &self.marked {
            if marked.contains(&offset) {
                return self.base;
            }
            if offset >= marked.end {
                offset - marked.len()
            } else {
                offset
            }
        } else {
            offset
        };
        let index = self
            .overlays
            .partition_point(|span| span.range.end <= committed);
        if let Some(span) = self
            .overlays
            .get(index)
            .filter(|span| span.range.contains(&committed))
        {
            emphasis = FontEmphasis {
                bold: span.bold.or(emphasis.bold),
                italic: span.italic.or(emphasis.italic),
            };
        }
        let index = self
            .named
            .partition_point(|(range, _)| range.end.0 <= committed);
        if let Some((_, role)) = self
            .named
            .get(index)
            .filter(|(range, _)| range.contains(&crate::text::ByteOffset(committed)))
        {
            let named = self
                .active_section_fonts()
                .filter(|fonts| fonts.range.contains(&crate::text::ByteOffset(committed)))
                .map_or(self.named_roles[*role as usize], |fonts| {
                    fonts.roles[*role as usize]
                });
            emphasis = FontEmphasis {
                bold: named.bold.or(emphasis.bold),
                italic: named.italic.or(emphasis.italic),
            };
        }
        emphasis
    }

    pub(crate) fn runs(&self, text: &str, start: usize, base: &gpui_kit::Font) -> Vec<TextRun> {
        if text.is_empty() {
            return Vec::new();
        }
        if self.is_uniform() {
            return vec![TextRun {
                len: text.len(),
                font: self.base.font(base),
                color: gpui_kit::black(),
                ..TextRun::default()
            }];
        }
        let mut runs: Vec<TextRun> = Vec::new();
        let mut previous = None;
        for (offset, grapheme) in text.grapheme_indices(true) {
            let emphasis = self.font_at(start + offset);
            if previous == Some(emphasis) {
                runs.last_mut().unwrap().len += grapheme.len();
            } else {
                runs.push(TextRun {
                    len: grapheme.len(),
                    font: emphasis.font(base),
                    color: gpui_kit::black(),
                    ..TextRun::default()
                });
                previous = Some(emphasis);
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::ByteOffset as B;
    use crate::theme::StyleOverride;

    #[test]
    fn current_section_cache_reuses_negative_checks_and_tracks_font_identity() {
        let named: Arc<[(Range<B>, StyleRole)]> = Arc::from([(B(0)..B(8), StyleRole::DiffContext)]);
        let make = |theme: &ThemeSettings,
                    range,
                    previous: Option<&Typography>,
                    named: &Arc<[(Range<B>, StyleRole)]>| {
            Typography::new(theme, None, None, 0, Arc::from([]))
                .with_named_ranges(named.clone(), theme)
                .with_current_section(Some(range), theme, previous)
        };
        let same_cache = |a: &Typography, b: &Typography| {
            Arc::ptr_eq(
                a.section_fonts.as_ref().unwrap(),
                b.section_fonts.as_ref().unwrap(),
            )
        };
        let mut theme = ThemeSettings::default();
        theme.set_style(
            StyleRole::DiffFile,
            StyleOverride::default()
                .with_text_attributes(StyleRole::DiffFile, Some("normal"), None, None, None)
                .unwrap(),
        );
        let first = make(&theme, B(0)..B(8), None, &named);
        assert!(first.active_section_fonts().is_none());
        let second = make(&theme, B(0)..B(8), Some(&first), &named);
        assert!(same_cache(&first, &second));
        let plain = Typography::new(&theme, None, None, 0, Arc::from([]))
            .with_named_ranges(named.clone(), &theme);
        assert_eq!(first, plain);

        theme.set_style(
            StyleRole::DiffContextHighlight,
            StyleOverride::new(
                StyleRole::DiffContextHighlight,
                Some("#123456"),
                Some("#234567"),
            )
            .unwrap(),
        );
        let recolored = make(&theme, B(0)..B(8), Some(&second), &named);
        assert!(same_cache(&second, &recolored));
        assert_eq!(second, recolored);

        theme.set_style(
            StyleRole::DiffContextHighlight,
            StyleOverride::default()
                .with_text_attributes(
                    StyleRole::DiffContextHighlight,
                    None,
                    Some(true),
                    None,
                    None,
                )
                .unwrap(),
        );
        let changed = make(&theme, B(0)..B(8), Some(&recolored), &named);
        assert!(!same_cache(&recolored, &changed));
        assert_ne!(recolored, changed);
        assert_eq!(changed.font_at(0).italic, Some(true));
        assert!(same_cache(
            &changed,
            &make(&theme, B(0)..B(8), Some(&changed), &named)
        ));
        assert!(!same_cache(
            &changed,
            &make(&theme, B(0)..B(4), Some(&changed), &named)
        ));
        let replacement = Arc::from(named.to_vec());
        assert!(!same_cache(
            &changed,
            &make(&theme, B(0)..B(8), Some(&changed), &replacement)
        ));
        theme.set_style(
            StyleRole::Default,
            StyleOverride::default()
                .with_text_attributes(StyleRole::Default, Some("bold"), None, None, None)
                .unwrap(),
        );
        assert!(!same_cache(
            &changed,
            &make(&theme, B(0)..B(8), Some(&changed), &named)
        ));
    }
}
