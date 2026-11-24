use std::collections::BTreeMap;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StyleRole {
    Default,
    Cursor,
    Region,
    IsearchMatch,
    IsearchLazy,
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
    SyntaxCommentDocumentation,
    SyntaxString,
    SyntaxStringEscape,
    SyntaxStringSpecial,
    SyntaxStringSpecialKey,
    SyntaxNumber,
    SyntaxType,
    SyntaxTypeBuiltin,
    SyntaxConstructor,
    SyntaxTag,
    SyntaxTagError,
    SyntaxFunction,
    SyntaxFunctionBuiltin,
    SyntaxFunctionMethod,
    SyntaxFunctionMacro,
    SyntaxFunctionCall,
    SyntaxFunctionMethodCall,
    SyntaxVariable,
    SyntaxVariableBuiltin,
    SyntaxVariableSpecial,
    SyntaxVariableParameter,
    SyntaxProperty,
    SyntaxAttribute,
    SyntaxLabel,
    SyntaxConstant,
    SyntaxConstantBuiltin,
    SyntaxBoolean,
    SyntaxOperator,
    SyntaxPunctuation,
    SyntaxPunctuationBracket,
    SyntaxPunctuationDelimiter,
    SyntaxPunctuationSpecial,
    MarkupRaw,
    MarkupLinkLabel,
    MarkupLinkUrl,
    MarkupHeading,
    MarkupStrong,
    MarkupEmphasis,
    DiffFile,
    DiffHunk,
    DiffAdded,
    DiffRemoved,
    SectionCurrent,
    SectionHeading,
    MagitBranch,
    MagitHash,
    DiffContext,
    DiffStatAdded,
    DiffStatRemoved,
    DiffFileHighlight,
    DiffHunkHighlight,
    DiffAddedHighlight,
    DiffRemovedHighlight,
    DiffContextHighlight,
    SearchFile,
    SearchLineNumber,
    SearchMatch,
    TreeRoot,
    TreeDirectory,
    TreeFile,
    TreeIgnored,
    TreeGitModified,
    TreeGitUntracked,
    TreeGitAdded,
    TreeGitConflict,
}

impl StyleRole {
    pub const COUNT: usize = Self::TreeGitConflict as usize + 1;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Default,
        Self::Cursor,
        Self::Region,
        Self::IsearchMatch,
        Self::IsearchLazy,
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
        Self::SyntaxCommentDocumentation,
        Self::SyntaxString,
        Self::SyntaxStringEscape,
        Self::SyntaxStringSpecial,
        Self::SyntaxStringSpecialKey,
        Self::SyntaxNumber,
        Self::SyntaxType,
        Self::SyntaxTypeBuiltin,
        Self::SyntaxConstructor,
        Self::SyntaxTag,
        Self::SyntaxTagError,
        Self::SyntaxFunction,
        Self::SyntaxFunctionBuiltin,
        Self::SyntaxFunctionMethod,
        Self::SyntaxFunctionMacro,
        Self::SyntaxFunctionCall,
        Self::SyntaxFunctionMethodCall,
        Self::SyntaxVariable,
        Self::SyntaxVariableBuiltin,
        Self::SyntaxVariableSpecial,
        Self::SyntaxVariableParameter,
        Self::SyntaxProperty,
        Self::SyntaxAttribute,
        Self::SyntaxLabel,
        Self::SyntaxConstant,
        Self::SyntaxConstantBuiltin,
        Self::SyntaxBoolean,
        Self::SyntaxOperator,
        Self::SyntaxPunctuation,
        Self::SyntaxPunctuationBracket,
        Self::SyntaxPunctuationDelimiter,
        Self::SyntaxPunctuationSpecial,
        Self::MarkupRaw,
        Self::MarkupLinkLabel,
        Self::MarkupLinkUrl,
        Self::MarkupHeading,
        Self::MarkupStrong,
        Self::MarkupEmphasis,
        Self::DiffFile,
        Self::DiffHunk,
        Self::DiffAdded,
        Self::DiffRemoved,
        Self::SectionCurrent,
        Self::SectionHeading,
        Self::MagitBranch,
        Self::MagitHash,
        Self::DiffContext,
        Self::DiffStatAdded,
        Self::DiffStatRemoved,
        Self::DiffFileHighlight,
        Self::DiffHunkHighlight,
        Self::DiffAddedHighlight,
        Self::DiffRemovedHighlight,
        Self::DiffContextHighlight,
        Self::SearchFile,
        Self::SearchLineNumber,
        Self::SearchMatch,
        Self::TreeRoot,
        Self::TreeDirectory,
        Self::TreeFile,
        Self::TreeIgnored,
        Self::TreeGitModified,
        Self::TreeGitUntracked,
        Self::TreeGitAdded,
        Self::TreeGitConflict,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Cursor => "cursor",
            Self::Region => "region",
            Self::IsearchMatch => "isearch",
            Self::IsearchLazy => "isearch.lazy",
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
            Self::SyntaxCommentDocumentation => "syntax.comment.documentation",
            Self::SyntaxString => "syntax.string",
            Self::SyntaxStringEscape => "syntax.string.escape",
            Self::SyntaxStringSpecial => "syntax.string.special",
            Self::SyntaxStringSpecialKey => "syntax.string.special.key",
            Self::SyntaxNumber => "syntax.number",
            Self::SyntaxType => "syntax.type",
            Self::SyntaxTypeBuiltin => "syntax.type.builtin",
            Self::SyntaxConstructor => "syntax.constructor",
            Self::SyntaxTag => "syntax.tag",
            Self::SyntaxTagError => "syntax.tag.error",
            Self::SyntaxFunction => "syntax.function",
            Self::SyntaxFunctionBuiltin => "syntax.function.builtin",
            Self::SyntaxFunctionMethod => "syntax.function.method",
            Self::SyntaxFunctionMacro => "syntax.function.macro",
            Self::SyntaxFunctionCall => "syntax.function.call",
            Self::SyntaxFunctionMethodCall => "syntax.function.method.call",
            Self::SyntaxVariable => "syntax.variable",
            Self::SyntaxVariableBuiltin => "syntax.variable.builtin",
            Self::SyntaxVariableSpecial => "syntax.variable.special",
            Self::SyntaxVariableParameter => "syntax.variable.parameter",
            Self::SyntaxProperty => "syntax.property",
            Self::SyntaxAttribute => "syntax.attribute",
            Self::SyntaxLabel => "syntax.label",
            Self::SyntaxConstant => "syntax.constant",
            Self::SyntaxConstantBuiltin => "syntax.constant.builtin",
            Self::SyntaxBoolean => "syntax.boolean",
            Self::SyntaxOperator => "syntax.operator",
            Self::SyntaxPunctuation => "syntax.punctuation",
            Self::SyntaxPunctuationBracket => "syntax.punctuation.bracket",
            Self::SyntaxPunctuationDelimiter => "syntax.punctuation.delimiter",
            Self::SyntaxPunctuationSpecial => "syntax.punctuation.special",
            Self::MarkupRaw => "markup.raw",
            Self::MarkupLinkLabel => "markup.link.label",
            Self::MarkupLinkUrl => "markup.link.url",
            Self::MarkupHeading => "markup.heading",
            Self::MarkupStrong => "markup.strong",
            Self::MarkupEmphasis => "markup.emphasis",
            Self::DiffFile => "diff.file",
            Self::DiffHunk => "diff.hunk",
            Self::DiffAdded => "diff.added",
            Self::DiffRemoved => "diff.removed",
            Self::SectionCurrent => "section.current",
            Self::SectionHeading => "section.heading",
            Self::MagitBranch => "magit.branch",
            Self::MagitHash => "magit.hash",
            Self::DiffContext => "diff.context",
            Self::DiffStatAdded => "diffstat.added",
            Self::DiffStatRemoved => "diffstat.removed",
            Self::DiffFileHighlight => "diff.file-highlight",
            Self::DiffHunkHighlight => "diff.hunk-highlight",
            Self::DiffAddedHighlight => "diff.added-highlight",
            Self::DiffRemovedHighlight => "diff.removed-highlight",
            Self::DiffContextHighlight => "diff.context-highlight",
            Self::SearchFile => "search.file",
            Self::SearchLineNumber => "search.line-number",
            Self::SearchMatch => "search.match",
            Self::TreeRoot => "tree.root",
            Self::TreeDirectory => "tree.directory",
            Self::TreeFile => "tree.file",
            Self::TreeIgnored => "tree.ignored",
            Self::TreeGitModified => "tree.git.modified",
            Self::TreeGitUntracked => "tree.git.untracked",
            Self::TreeGitAdded => "tree.git.added",
            Self::TreeGitConflict => "tree.git.conflict",
        }
    }

    pub(crate) fn highlighted(self) -> Self {
        match self {
            Self::DiffFile => Self::DiffFileHighlight,
            Self::DiffHunk => Self::DiffHunkHighlight,
            Self::DiffAdded => Self::DiffAddedHighlight,
            Self::DiffRemoved => Self::DiffRemovedHighlight,
            Self::DiffContext => Self::DiffContextHighlight,
            _ => self,
        }
    }

    fn plain(self) -> Option<Self> {
        match self {
            Self::DiffFileHighlight => Some(Self::DiffFile),
            Self::DiffHunkHighlight => Some(Self::DiffHunk),
            Self::DiffAddedHighlight => Some(Self::DiffAdded),
            Self::DiffRemovedHighlight => Some(Self::DiffRemoved),
            Self::DiffContextHighlight => Some(Self::DiffContext),
            _ => None,
        }
    }

    fn syntax_parent(self) -> Option<Self> {
        match self {
            Self::SyntaxCommentDocumentation => Some(Self::SyntaxComment),
            Self::SyntaxStringEscape | Self::SyntaxStringSpecial => Some(Self::SyntaxString),
            Self::SyntaxTypeBuiltin | Self::SyntaxConstructor => Some(Self::SyntaxType),
            Self::SyntaxTagError => Some(Self::SyntaxTag),
            Self::SyntaxFunctionBuiltin
            | Self::SyntaxFunctionMethod
            | Self::SyntaxFunctionMacro => Some(Self::SyntaxFunction),
            Self::SyntaxFunctionMethodCall => Some(Self::SyntaxFunctionCall),
            Self::SyntaxVariableBuiltin
            | Self::SyntaxVariableSpecial
            | Self::SyntaxVariableParameter => Some(Self::SyntaxVariable),
            Self::SyntaxConstantBuiltin => Some(Self::SyntaxConstant),
            Self::SyntaxBoolean => Some(Self::SyntaxConstantBuiltin),
            Self::SyntaxPunctuationBracket
            | Self::SyntaxPunctuationDelimiter
            | Self::SyntaxPunctuationSpecial => Some(Self::SyntaxPunctuation),
            Self::MarkupRaw | Self::MarkupLinkUrl => Some(Self::SyntaxString),
            _ => None,
        }
    }

    pub(crate) fn is_markup_modifier(self) -> bool {
        matches!(
            self,
            Self::MarkupHeading | Self::MarkupStrong | Self::MarkupEmphasis
        )
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
                    | Self::IsearchMatch
                    | Self::IsearchLazy
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
    pub(crate) fn emphasis(self, base: Self) -> Self {
        Self {
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
        }
    }

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
        let mut settings = Self {
            themes,
            active: "light".into(),
            overrides: BTreeMap::new(),
            resolved: builtin_colors(ThemeMode::Light),
            attributes: builtin_attributes(ThemeMode::Light),
        };
        settings.resolve();
        settings
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
        if role == StyleRole::SectionCurrent {
            attributes.background = Some(self.style(role).background);
        }
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
        for role in StyleRole::ALL {
            if let Some(parent) = role.syntax_parent() {
                debug_assert!(parent < role);
                self.resolved[role as usize] = self.resolved[parent as usize];
                self.attributes[role as usize] = self.attributes[parent as usize];
            } else if let Some(base) = role.plain() {
                self.resolved[role as usize] = self.resolved[base as usize];
                self.attributes[role as usize] = self.attributes[base as usize];
                let background = self.style(StyleRole::SectionCurrent).background;
                self.resolved[role as usize].background = background;
                self.attributes[role as usize].background = Some(background);
                if matches!(
                    role,
                    StyleRole::DiffFileHighlight | StyleRole::DiffHunkHighlight
                ) {
                    self.attributes[role as usize].bold = Some(true);
                }
            }
            for styles in [&definition.styles, &self.overrides] {
                if let Some(colors) = styles.get(&role) {
                    colors.apply(&mut self.resolved[role as usize]);
                    self.attributes[role as usize].merge(*colors);
                }
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
    StyleRole::ALL.map(|role| match role {
        StyleRole::Default
        | StyleRole::Completion
        | StyleRole::SyntaxFunctionCall
        | StyleRole::SyntaxFunctionMethodCall
        | StyleRole::SyntaxVariable
        | StyleRole::SyntaxVariableBuiltin
        | StyleRole::SyntaxVariableSpecial
        | StyleRole::SyntaxVariableParameter
        | StyleRole::SyntaxProperty
        | StyleRole::SyntaxAttribute
        | StyleRole::SyntaxLabel
        | StyleRole::MarkupLinkLabel
        | StyleRole::SyntaxOperator
        | StyleRole::MarkupEmphasis
        | StyleRole::DiffContext
        | StyleRole::DiffContextHighlight
        | StyleRole::TreeFile => pair(foreground, background),
        StyleRole::Cursor => pair(background, foreground),
        StyleRole::Region => pair(foreground, selection),
        StyleRole::IsearchMatch => {
            if mode == ThemeMode::Dark {
                pair(0x202124, 0xffcc66)
            } else {
                pair(0xffffff, 0xb0006d)
            }
        }
        StyleRole::IsearchLazy => {
            if mode == ThemeMode::Dark {
                pair(0xe6e6e6, 0x3b4d6b)
            } else {
                pair(0x202124, 0xd8ecff)
            }
        }
        StyleRole::ModeLine | StyleRole::CompletionSelected => pair(foreground, active_bg),
        StyleRole::ModeLineInactive => pair(muted, inactive_bg),
        StyleRole::MinibufferPrompt | StyleRole::CompletionMatch => pair(accent, background),
        StyleRole::EchoArea
        | StyleRole::CompletionAnnotation
        | StyleRole::SyntaxPunctuation
        | StyleRole::SyntaxPunctuationBracket
        | StyleRole::SyntaxPunctuationDelimiter
        | StyleRole::SyntaxPunctuationSpecial
        | StyleRole::MagitHash => pair(muted, background),
        StyleRole::Separator => pair(separator, background),
        StyleRole::SyntaxKeyword | StyleRole::SectionHeading => pair(keyword, background),
        StyleRole::SyntaxComment
        | StyleRole::SyntaxCommentDocumentation
        | StyleRole::TreeIgnored => pair(comment, background),
        StyleRole::SyntaxString
        | StyleRole::SyntaxStringEscape
        | StyleRole::SyntaxStringSpecial
        | StyleRole::MarkupRaw
        | StyleRole::MarkupLinkUrl
        | StyleRole::DiffStatAdded
        | StyleRole::TreeGitUntracked => pair(string, background),
        StyleRole::SyntaxNumber | StyleRole::SearchLineNumber | StyleRole::TreeGitModified => {
            pair(number, background)
        }
        StyleRole::SyntaxType
        | StyleRole::SyntaxTypeBuiltin
        | StyleRole::SyntaxConstructor
        | StyleRole::SyntaxTag
        | StyleRole::SyntaxTagError
        | StyleRole::MarkupHeading
        | StyleRole::MarkupStrong
        | StyleRole::DiffFile
        | StyleRole::DiffFileHighlight
        | StyleRole::MagitBranch
        | StyleRole::SearchFile
        | StyleRole::TreeRoot
        | StyleRole::TreeGitAdded => pair(type_color, background),
        StyleRole::SearchMatch => pair(
            accent,
            if mode == ThemeMode::Dark {
                0x3a3d1e
            } else {
                0xfff2a8
            },
        ),
        StyleRole::SyntaxFunction
        | StyleRole::SyntaxFunctionBuiltin
        | StyleRole::SyntaxFunctionMethod
        | StyleRole::SyntaxFunctionMacro
        | StyleRole::SyntaxStringSpecialKey
        | StyleRole::TreeDirectory => pair(function, background),
        StyleRole::SyntaxConstant
        | StyleRole::SyntaxConstantBuiltin
        | StyleRole::SyntaxBoolean
        | StyleRole::DiffStatRemoved
        | StyleRole::TreeGitConflict => pair(constant, background),
        StyleRole::DiffHunk | StyleRole::DiffHunkHighlight => pair(function, inactive_bg),
        StyleRole::DiffAdded | StyleRole::DiffAddedHighlight => pair(
            string,
            if mode == ThemeMode::Dark {
                0x20372a
            } else {
                0xe5f4e8
            },
        ),
        StyleRole::DiffRemoved | StyleRole::DiffRemovedHighlight => pair(
            constant,
            if mode == ThemeMode::Dark {
                0x42292d
            } else {
                0xfce8e6
            },
        ),
        StyleRole::SectionCurrent => pair(
            foreground,
            if mode == ThemeMode::Dark {
                0x303a4a
            } else {
                0xeaf0f8
            },
        ),
    })
}

fn builtin_attributes(mode: ThemeMode) -> [StyleOverride; StyleRole::COUNT] {
    let mut attributes = [StyleOverride::default(); StyleRole::COUNT];
    attributes[StyleRole::SyntaxKeyword as usize].bold = Some(true);
    attributes[StyleRole::SyntaxType as usize].italic = Some(true);
    attributes[StyleRole::SyntaxVariable as usize].bold = Some(true);
    attributes[StyleRole::MarkupHeading as usize].bold = Some(true);
    attributes[StyleRole::MarkupStrong as usize].bold = Some(true);
    attributes[StyleRole::DiffFile as usize].bold = Some(true);
    attributes[StyleRole::DiffHunk as usize].bold = Some(true);
    attributes[StyleRole::SectionHeading as usize].bold = Some(true);
    attributes[StyleRole::SearchFile as usize].bold = Some(true);
    attributes[StyleRole::SearchMatch as usize].bold = Some(true);
    attributes[StyleRole::TreeRoot as usize].bold = Some(true);
    attributes[StyleRole::TreeGitConflict as usize].bold = Some(true);
    for role in [
        StyleRole::DiffAdded,
        StyleRole::DiffRemoved,
        StyleRole::DiffHunk,
        StyleRole::SearchMatch,
    ] {
        attributes[role as usize].background = Some(builtin_colors(mode)[role as usize].background);
    }
    attributes[StyleRole::MarkupEmphasis as usize].italic = Some(true);
    attributes
}

#[cfg(test)]
mod tests;
