semacs.registerWorkflowCommand("magit-stage", {
    modes: ["magit-status"],
    run: context => {
        if (!context.status?.selection?.canStage)
            throw new Error("Select one supported unstaged or untracked file; refresh stale status first");
        return { operation: "magit.stage" };
    },
});
semacs.registerWorkflowCommand("magit-unstage", {
    modes: ["magit-status"],
    run: context => {
        if (!context.status?.selection?.canUnstage)
            throw new Error("Select one supported staged file; refresh stale status first");
        return { operation: "magit.unstage" };
    },
});
