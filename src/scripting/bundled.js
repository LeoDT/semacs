semacs.registerWorkflowCommand("magit-stage", {
    modes: ["magit-status"],
    run: context => {
        if (!context.status?.selection?.canStage)
            throw new Error("Select a supported unstaged/untracked section, file, or hunk; refresh stale status first");
        return { operation: "magit.stage" };
    },
});
semacs.registerWorkflowCommand("magit-unstage", {
    modes: ["magit-status"],
    run: context => {
        if (!context.status?.selection?.canUnstage)
            throw new Error("Select a supported staged section, file, or hunk; refresh stale status first");
        return { operation: "magit.unstage" };
    },
});
