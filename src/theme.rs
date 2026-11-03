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
}

impl StyleRole {
    pub const COUNT: usize = 22;
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
        }
    }

    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|role| role.name() == name)
            .ok_or_else(|| format!("Unknown style: {name}"))
    }

    fn supports_foreground(self) -> bool {
        self != Self::Region
    }

    fn supports_background(self) -> bool {
        matches!(
            self,
            Self::Default
                | Self::Cursor
                | Self::Region
                | Self::ModeLine
                | Self::ModeLineInactive
                | Self::EchoArea
                | Self::Completion
                | Self::CompletionSelected
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleColors {
    pub foreground: Hsla,
    pub background: Hsla,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StyleOverride {
    foreground: Option<Hsla>,
    background: Option<Hsla>,
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
        })
    }

    fn merge(&mut self, next: Self) {
        self.foreground = next.foreground.or(self.foreground);
        self.background = next.background.or(self.background);
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
        for styles in [&definition.styles, &self.overrides] {
            for (role, colors) in styles {
                colors.apply(&mut self.resolved[*role as usize]);
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
    ]
}

#[cfg(test)]
mod tests;
