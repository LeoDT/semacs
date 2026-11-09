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
    | "diff.file" | "diff.hunk" | "diff.added" | "diff.removed" | "diff.context"
    | "diff.file-highlight" | "diff.hunk-highlight" | "diff.added-highlight"
    | "diff.removed-highlight" | "diff.context-highlight"
    | "diffstat.added" | "diffstat.removed" | "section.current" | "section.heading"
    | "magit.branch" | "magit.hash";

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
    N extends "default" | "minibuffer-prompt" | "section.heading" | `syntax.${string}` | `markup.${string}` | `diff.${string}` | `diffstat.${string}` | `magit.${string}` ? SemacsTextStyle :
    N extends "completion-match" | "completion-annotation" | "separator" ? { foreground?: string } :
    { foreground?: string; background?: string };

type SemacsThemeStyles = { [N in SemacsStyleName]?: SemacsStyleColors<N> };

interface SemacsThemeDefinition {
    appearance: "light" | "dark";
    styles: SemacsThemeStyles;
}

type SemacsMode = "fundamental" | "magit-diff" | "magit-status" | "git-commit" | "magit-process";

interface SemacsStatusSelection {
    readonly group: "untracked" | "unstaged" | "staged" | "unmerged";
    readonly canStage: boolean;
    readonly canUnstage: boolean;
}

interface SemacsWorkflowContext {
    readonly mode: SemacsMode;
    readonly status: { readonly selection: SemacsStatusSelection | null } | null;
}

type SemacsWorkflowRequest =
    | { operation: "magit.status.open" }
    | { operation: "magit.status.refresh" }
    | { operation: "magit.diff.open"; comparison: "staged" | "unstaged" }
    | { operation: "magit.stage" }
    | { operation: "magit.unstage" };

interface SemacsWorkflowCommandDefinition {
    modes?: readonly SemacsMode[];
    run(context: SemacsWorkflowContext): SemacsWorkflowRequest | null;
}

interface SemacsConfigurationApi {
    defineTheme(name: string, definition: SemacsThemeDefinition): void;
    loadTheme(name: string): void;
    setStyle<N extends SemacsStyleName>(name: N, colors: SemacsStyleColors<N>): void;
    setFont(settings: SemacsFontSettings): void;
    registerCommand(name: string, transform: (text: string) => string): void;
    registerWorkflowCommand(name: string, definition: SemacsWorkflowCommandDefinition): void;
    bindKey(keys: string, commandName: string | null, options?: {mode: SemacsMode}): void;
}

declare const semacs: Readonly<SemacsConfigurationApi>;
