use std::collections::BTreeMap;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StyleRole {
    Default,
    Cursor,
    Region,
    ModeLine,
    ModeLineInactive,
    MinibufferPrompt,
    EchoArea,
    Completion,
    CompletionSelected,
    CompletionMatch,
    CompletionAnnotation,
    Separator,
    SyntaxKeyword,
    SyntaxComment,
    SyntaxString,
    SyntaxNumber,
    SyntaxType,
    SyntaxFunction,
    SyntaxVariable,
    SyntaxConstant,
    SyntaxOperator,
    SyntaxPunctuation,
    MarkupHeading,
    MarkupStrong,
    MarkupEmphasis,
    DiffFile,
    DiffHunk,
    DiffAdded,
    DiffRemoved,
    SectionCurrent,
}

impl StyleRole {
    pub const COUNT: usize = 30;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Default,
        Self::Cursor,
        Self::Region,
        Self::ModeLine,
        Self::ModeLineInactive,
        Self::MinibufferPrompt,
        Self::EchoArea,
        Self::Completion,
        Self::CompletionSelected,
        Self::CompletionMatch,
        Self::CompletionAnnotation,
        Self::Separator,
        Self::SyntaxKeyword,
        Self::SyntaxComment,
        Self::SyntaxString,
        Self::SyntaxNumber,
        Self::SyntaxType,
        Self::SyntaxFunction,
        Self::SyntaxVariable,
        Self::SyntaxConstant,
        Self::SyntaxOperator,
        Self::SyntaxPunctuation,
        Self::MarkupHeading,
        Self::MarkupStrong,
        Self::MarkupEmphasis,
        Self::DiffFile,
        Self::DiffHunk,
        Self::DiffAdded,
        Self::DiffRemoved,
        Self::SectionCurrent,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Cursor => "cursor",
            Self::Region => "region",
            Self::ModeLine => "mode-line",
            Self::ModeLineInactive => "mode-line-inactive",
            Self::MinibufferPrompt => "minibuffer-prompt",
            Self::EchoArea => "echo-area",
            Self::Completion => "completion",
            Self::CompletionSelected => "completion-selected",
            Self::CompletionMatch => "completion-match",
            Self::CompletionAnnotation => "completion-annotation",
            Self::Separator => "separator",
            Self::SyntaxKeyword => "syntax.keyword",
            Self::SyntaxComment => "syntax.comment",
            Self::SyntaxString => "syntax.string",
            Self::SyntaxNumber => "syntax.number",
            Self::SyntaxType => "syntax.type",
            Self::SyntaxFunction => "syntax.function",
            Self::SyntaxVariable => "syntax.variable",
            Self::SyntaxConstant => "syntax.constant",
            Self::SyntaxOperator => "syntax.operator",
            Self::SyntaxPunctuation => "syntax.punctuation",
            Self::MarkupHeading => "markup.heading",
            Self::MarkupStrong => "markup.strong",
            Self::MarkupEmphasis => "markup.emphasis",
            Self::DiffFile => "diff.file",
            Self::DiffHunk => "diff.hunk",
            Self::DiffAdded => "diff.added",
            Self::DiffRemoved => "diff.removed",
            Self::SectionCurrent => "section.current",
        }
    }

    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|role| role.name() == name)
            .ok_or_else(|| format!("Unknown style: {name}"))
    }

    fn supports_foreground(self) -> bool {
        !matches!(self, Self::Region | Self::SectionCurrent)
    }

    fn supports_background(self) -> bool {
        self.supports_text_attributes()
            || matches!(
                self,
                Self::Default
                    | Self::Cursor
                    | Self::Region
                    | Self::SectionCurrent
                    | Self::ModeLine
                    | Self::ModeLineInactive
                    | Self::EchoArea
                    | Self::Completion
                    | Self::CompletionSelected
            )
    }

    pub fn supports_text_attributes(self) -> bool {
        self == Self::Default
            || self == Self::MinibufferPrompt
            || (self >= Self::SyntaxKeyword && self != Self::SectionCurrent)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontEmphasis {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

impl FontEmphasis {
    pub fn font(self, base: &gpui_kit::Font) -> gpui_kit::Font {
        let mut font = base.clone();
        if let Some(bold) = self.bold {
            font.weight = if bold {
                gpui_kit::FontWeight::BOLD
            } else {
                gpui_kit::FontWeight::NORMAL
            };
        }
        if let Some(italic) = self.italic {
            font.style = if italic {
                gpui_kit::FontStyle::Italic
            } else {
                gpui_kit::FontStyle::Normal
            };
        }
        font
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleColors {
    pub foreground: Hsla,
    pub background: Hsla,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StyleOverride {
    pub(crate) foreground: Option<Hsla>,
    pub(crate) background: Option<Hsla>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) underline: Option<bool>,
    pub(crate) strikethrough: Option<bool>,
}

impl StyleOverride {
    pub fn new(
        role: StyleRole,
        foreground: Option<&str>,
        background: Option<&str>,
    ) -> Result<Self, String> {
        if foreground.is_some() && !role.supports_foreground()
            || background.is_some() && !role.supports_background()
        {
            return Err(format!("Unsupported color attribute for {}", role.name()));
        }
        Ok(Self {
            foreground: foreground.map(parse_color).transpose()?,
            background: background.map(parse_color).transpose()?,
            ..Self::default()
        })
    }

    pub fn with_text_attributes(
        mut self,
        role: StyleRole,
        weight: Option<&str>,
        italic: Option<bool>,
        underline: Option<bool>,
        strikethrough: Option<bool>,
    ) -> Result<Self, String> {
        if !role.supports_text_attributes()
            && (weight.is_some()
                || italic.is_some()
                || underline.is_some()
                || strikethrough.is_some())
        {
            return Err(format!("Unsupported text attribute for {}", role.name()));
        }
        self.bold = weight
            .map(|weight| match weight {
                "normal" => Ok(false),
                "bold" => Ok(true),
                _ => Err(format!("Weight must be normal or bold: {weight}")),
            })
            .transpose()?;
        self.italic = italic;
        self.underline = underline;
        self.strikethrough = strikethrough;
        Ok(self)
    }

    pub(crate) fn merge(&mut self, next: Self) {
        self.foreground = next.foreground.or(self.foreground);
        self.background = next.background.or(self.background);
        self.bold = next.bold.or(self.bold);
        self.italic = next.italic.or(self.italic);
        self.underline = next.underline.or(self.underline);
        self.strikethrough = next.strikethrough.or(self.strikethrough);
    }

    pub(crate) fn emphasis(self, base: FontEmphasis) -> FontEmphasis {
        FontEmphasis {
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
        }
    }

    fn apply(self, colors: &mut StyleColors) {
        if let Some(color) = self.foreground {
            colors.foreground = color;
        }
        if let Some(color) = self.background {
            colors.background = color;
        }
    }
}

fn parse_color(value: &str) -> Result<Hsla, String> {
    let digits = value
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("Color must be #RRGGBB: {value}"))?;
    Ok(rgb(u32::from_str_radix(digits, 16).expect("validated hex color")).into())
}

#[derive(Clone, Debug, PartialEq)]
struct ThemeDefinition {
    mode: ThemeMode,
    styles: BTreeMap<StyleRole, StyleOverride>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSettings {
    themes: BTreeMap<String, ThemeDefinition>,
    active: String,
    overrides: BTreeMap<StyleRole, StyleOverride>,
    resolved: [StyleColors; StyleRole::COUNT],
    attributes: [StyleOverride; StyleRole::COUNT],
}

impl Global for ThemeSettings {}

impl Default for ThemeSettings {
    fn default() -> Self {
        let mut themes = BTreeMap::new();
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            themes.insert(
                name.into(),
                ThemeDefinition {
                    mode,
                    styles: BTreeMap::new(),
                },
            );
        }
        Self {
            themes,
            active: "light".into(),
            overrides: BTreeMap::new(),
            resolved: builtin_colors(ThemeMode::Light),
            attributes: builtin_attributes(ThemeMode::Light),
        }
    }
}

impl ThemeSettings {
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn active(&self) -> &str {
        &self.active
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.themes.keys().map(String::as_str)
    }

    pub fn style(&self, role: StyleRole) -> StyleColors {
        self.resolved[role as usize]
    }

    pub(crate) fn attributes(&self, role: StyleRole) -> StyleOverride {
        let mut attributes = self.attributes[role as usize];
        attributes.foreground = Some(self.style(role).foreground);
        attributes
    }

    pub(crate) fn patch(&self, role: StyleRole) -> StyleOverride {
        self.attributes[role as usize]
    }

    pub fn define(
        &mut self,
        name: &str,
        appearance: &str,
        styles: Vec<(StyleRole, StyleOverride)>,
    ) -> Result<(), String> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(
                "Theme name must be 1–64 lowercase ASCII letters, digits or hyphens".into(),
            );
        }
        if self.themes.contains_key(name) {
            return Err(format!("Duplicate or reserved theme: {name}"));
        }
        if self.themes.len() >= 34 {
            return Err("At most 32 custom themes are allowed".into());
        }
        let mode = match appearance {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => return Err("Theme appearance must be light or dark".into()),
        };
        self.themes.insert(
            name.into(),
            ThemeDefinition {
                mode,
                styles: styles.into_iter().collect(),
            },
        );
        Ok(())
    }

    pub fn set_style(&mut self, role: StyleRole, colors: StyleOverride) {
        self.overrides.entry(role).or_default().merge(colors);
        self.resolve();
    }

    pub fn select(&mut self, name: &str) -> Result<(), String> {
        if !self.themes.contains_key(name) {
            return Err(format!("Unknown theme: {name}"));
        }
        self.active = name.into();
        self.resolve();
        Ok(())
    }

    fn resolve(&mut self) {
        let definition = &self.themes[&self.active];
        self.resolved = builtin_colors(definition.mode);
        self.attributes = builtin_attributes(definition.mode);
        for styles in [&definition.styles, &self.overrides] {
            for (role, colors) in styles {
                colors.apply(&mut self.resolved[*role as usize]);
                self.attributes[*role as usize].merge(*colors);
            }
        }
    }

    pub fn install(self, cx: &mut App) {
        let mode = self.themes[&self.active].mode;
        Theme::change(mode, None, cx);
        let default = self.style(StyleRole::Default);
        let selected = self.style(StyleRole::CompletionSelected);
        Theme::update(cx, |theme| {
            theme.colors.foreground = default.foreground;
            theme.colors.background = default.background;
            theme.colors.selection = self.style(StyleRole::Region).background;
            theme.colors.caret = self.style(StyleRole::Cursor).background;
            theme.colors.accent = selected.background;
            theme.colors.accent_foreground = selected.foreground;
            theme.colors.primary = self.style(StyleRole::CompletionMatch).foreground;
            theme.colors.muted_foreground = self.style(StyleRole::CompletionAnnotation).foreground;
            theme.colors.border = self.style(StyleRole::Separator).foreground;
        });
        cx.set_global(self);
    }
}

fn builtin_colors(mode: ThemeMode) -> [StyleColors; StyleRole::COUNT] {
    let (foreground, background, muted, active_bg, inactive_bg, selection, accent, separator) =
        if mode == ThemeMode::Dark {
            (
                0xe6e6e6, 0x202124, 0xa6abb3, 0x39465a, 0x292c32, 0x3d5275, 0x8ab4f8, 0x50545c,
            )
        } else {
            (
                0x202124, 0xffffff, 0x626873, 0xdce5f2, 0xf0f1f3, 0xc5d9f5, 0x174ea6, 0xb9bec7,
            )
        };
    let (keyword, comment, string, number, type_color, function, constant) =
        if mode == ThemeMode::Dark {
            (
                0xc792ea, 0x9aa0a6, 0xa5d6a7, 0xffcc80, 0x80cbc4, 0x82b1ff, 0xf4b4c4,
            )
        } else {
            (
                0x7b1fa2, 0x626873, 0x2e7d32, 0x945500, 0x00695c, 0x174ea6, 0x9c254d,
            )
        };
    let pair = |fg, bg| StyleColors {
        foreground: rgb(fg).into(),
        background: rgb(bg).into(),
    };
    [
        pair(foreground, background),
        pair(background, foreground),
        pair(foreground, selection),
        pair(foreground, active_bg),
        pair(muted, inactive_bg),
        pair(accent, background),
        pair(muted, background),
        pair(foreground, background),
        pair(foreground, active_bg),
        pair(accent, background),
        pair(muted, background),
        pair(separator, background),
        pair(keyword, background),
        pair(comment, background),
        pair(string, background),
        pair(number, background),
        pair(type_color, background),
        pair(function, background),
        pair(foreground, background),
        pair(constant, background),
        pair(foreground, background),
        pair(muted, background),
        pair(type_color, background),
        pair(type_color, background),
        pair(foreground, background),
        pair(type_color, background),
        pair(function, background),
        pair(
            string,
            if mode == ThemeMode::Dark {
                0x20372a
            } else {
                0xe5f4e8
            },
        ),
        pair(
            constant,
            if mode == ThemeMode::Dark {
                0x42292d
            } else {
                0xfce8e6
            },
        ),
        pair(
            foreground,
            if mode == ThemeMode::Dark {
                0x303a4a
            } else {
                0xeaf0f8
            },
        ),
    ]
}

fn builtin_attributes(mode: ThemeMode) -> [StyleOverride; StyleRole::COUNT] {
    let mut attributes = [StyleOverride::default(); StyleRole::COUNT];
    attributes[StyleRole::MarkupHeading as usize].bold = Some(true);
    attributes[StyleRole::MarkupStrong as usize].bold = Some(true);
    attributes[StyleRole::DiffFile as usize].bold = Some(true);
    attributes[StyleRole::DiffHunk as usize].bold = Some(true);
    for role in [StyleRole::DiffAdded, StyleRole::DiffRemoved] {
        attributes[role as usize].background = Some(builtin_colors(mode)[role as usize].background);
    }
    attributes[StyleRole::MarkupEmphasis as usize].italic = Some(true);
    attributes
}

#[cfg(test)]
mod tests;
