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
    | "syntax.operator" | "syntax.punctuation";

type SemacsStyleColors<N extends SemacsStyleName> =
    N extends "region" ? { background?: string } :
    N extends "minibuffer-prompt" | "completion-match" | "completion-annotation" | "separator" | `syntax.${string}`
        ? { foreground?: string } :
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
    bindKey(keys: string, commandName: string | null): void;
}

declare const semacs: Readonly<SemacsConfigurationApi>;
