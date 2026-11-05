interface SemacsFontSettings {
    family?: string;
    size?: number;
}

type SemacsStyleName =
    | "default" | "cursor" | "region" | "mode-line" | "mode-line-inactive"
    | "minibuffer-prompt" | "echo-area" | "completion" | "completion-selected"
    | "completion-match" | "completion-annotation" | "separator"
    | "syntax.keyword" | "syntax.comment" | "syntax.string" | "syntax.number"
    | "syntax.type" | "syntax.function" | "syntax.variable" | "syntax.constant"
    | "syntax.operator" | "syntax.punctuation"
    | "markup.heading" | "markup.strong" | "markup.emphasis"
    | "diff.file" | "diff.hunk" | "diff.added" | "diff.removed" | "section.current";

interface SemacsTextStyle {
    foreground?: string;
    background?: string;
    weight?: "normal" | "bold";
    italic?: boolean;
    underline?: boolean;
    strikethrough?: boolean;
}

type SemacsStyleColors<N extends SemacsStyleName> =
    N extends "region" | "section.current" ? { background?: string } :
    N extends "default" | "minibuffer-prompt" | `syntax.${string}` | `markup.${string}` | `diff.${string}` ? SemacsTextStyle :
    N extends "completion-match" | "completion-annotation" | "separator" ? { foreground?: string } :
    { foreground?: string; background?: string };

type SemacsThemeStyles = { [N in SemacsStyleName]?: SemacsStyleColors<N> };

interface SemacsThemeDefinition {
    appearance: "light" | "dark";
    styles: SemacsThemeStyles;
}

interface SemacsConfigurationApi {
    defineTheme(name: string, definition: SemacsThemeDefinition): void;
    loadTheme(name: string): void;
    setStyle<N extends SemacsStyleName>(name: N, colors: SemacsStyleColors<N>): void;
    setFont(settings: SemacsFontSettings): void;
    registerCommand(name: string, transform: (text: string) => string): void;
    bindKey(keys: string, commandName: string | null, options?: {mode: string}): void;
}

declare const semacs: Readonly<SemacsConfigurationApi>;
