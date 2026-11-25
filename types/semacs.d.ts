interface SemacsFontSettings {
    family?: string;
    size?: number;
}

interface SemacsAutoRevertSettings {
    enabled?: boolean;
    intervalMs?: number;
    verbose?: boolean;
}

type SemacsStyleName =
    | "default" | "cursor" | "region" | "isearch" | "isearch.lazy" | "mode-line" | "mode-line-inactive"
    | "minibuffer-prompt" | "echo-area" | "completion" | "completion-selected"
    | "completion-match" | "completion-annotation" | "separator"
    | "syntax.keyword" | "syntax.comment" | "syntax.comment.documentation"
    | "syntax.string" | "syntax.string.escape" | "syntax.string.special" | "syntax.string.special.key"
    | "syntax.number"
    | "syntax.type" | "syntax.type.builtin" | "syntax.constructor" | "syntax.tag" | "syntax.tag.error"
    | "syntax.function" | "syntax.function.builtin" | "syntax.function.method" | "syntax.function.macro"
    | "syntax.function.call" | "syntax.function.method.call"
    | "syntax.variable" | "syntax.variable.builtin" | "syntax.variable.special" | "syntax.variable.parameter"
    | "syntax.property" | "syntax.attribute" | "syntax.label"
    | "syntax.constant" | "syntax.constant.builtin" | "syntax.boolean"
    | "syntax.operator" | "syntax.punctuation"
    | "syntax.punctuation.bracket" | "syntax.punctuation.delimiter" | "syntax.punctuation.special"
    | "markup.heading" | "markup.strong" | "markup.emphasis"
    | "markup.raw" | "markup.link.label" | "markup.link.url"
    | "diff.file" | "diff.hunk" | "diff.added" | "diff.removed" | "diff.context"
    | "diff.file-highlight" | "diff.hunk-highlight" | "diff.added-highlight"
    | "diff.removed-highlight" | "diff.context-highlight"
    | "diffstat.added" | "diffstat.removed" | "section.current" | "section.heading"
    | "magit.branch" | "magit.hash"
    | "search.file" | "search.line-number" | "search.match"
    | "tree.root" | "tree.directory" | "tree.file" | "tree.ignored"
    | "tree.git.modified" | "tree.git.untracked" | "tree.git.added" | "tree.git.conflict";

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
    N extends "default" | "minibuffer-prompt" | "section.heading" | `syntax.${string}` | `markup.${string}` | `diff.${string}` | `diffstat.${string}` | `magit.${string}` | `search.${string}` | `tree.${string}` ? SemacsTextStyle :
    N extends "completion-match" | "completion-annotation" | "separator" ? { foreground?: string } :
    { foreground?: string; background?: string };

type SemacsThemeStyles = { [N in SemacsStyleName]?: SemacsStyleColors<N> };

interface SemacsThemeDefinition {
    appearance: "light" | "dark";
    styles: SemacsThemeStyles;
}

type SemacsMode = "fundamental" | "treemacs" | "magit-diff" | "magit-status" | "git-commit" | "magit-process" | "magit-log" | "magit-log-select" | "magit-revision" | "rg";

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
    setAutoRevert(settings: SemacsAutoRevertSettings): void;
    registerCommand(name: string, transform: (text: string) => string): void;
    registerWorkflowCommand(name: string, definition: SemacsWorkflowCommandDefinition): void;
    bindKey(keys: string, commandName: string | null, options?: {mode: SemacsMode}): void;
}

declare const semacs: Readonly<SemacsConfigurationApi>;
