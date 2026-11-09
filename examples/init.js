/// <reference path="../types/semacs.d.ts" />

semacs.setFont({ size: 18 });

semacs.defineTheme("night", {
    appearance: "dark",
    styles: {
        "mode-line": { foreground: "#ffffff", background: "#304060" },
        "minibuffer-prompt": { foreground: "#a8c7fa" },
        "section.heading": { foreground: "#c792ea", weight: "bold" },
        "diff.hunk": { foreground: "#e6e6e6", background: "#292c32" },
        "diff.hunk-highlight": { background: "#39465a" },
    },
});
semacs.loadTheme("night");
semacs.setStyle("cursor", { foreground: "#202124", background: "#ffcc66" });

semacs.registerCommand("uppercase-region", text => text.toUpperCase());
semacs.registerCommand("lowercase-region", text => text.toLowerCase());

semacs.bindKey("ctrl-c u", "uppercase-region");
semacs.bindKey("ctrl-c U", "uppercase-region");
semacs.bindKey("ctrl-c l", "lowercase-region");

semacs.registerWorkflowCommand("status-or-refresh", {
    run: context => context.status
        ? { operation: "magit.status.refresh" }
        : { operation: "magit.status.open" },
});
semacs.bindKey("ctrl-c g", "status-or-refresh");

// Bundled magit-stage / magit-unstage already use s / u in status mode.
semacs.bindKey("S", "magit-stage", { mode: "magit-status" });
