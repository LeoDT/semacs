/// <reference path="../types/semacs.d.ts" />

semacs.setFont({ size: 18 });

semacs.defineTheme("night", {
    appearance: "dark",
    styles: {
        "mode-line": { foreground: "#ffffff", background: "#304060" },
        "minibuffer-prompt": { foreground: "#a8c7fa" },
    },
});
semacs.loadTheme("night");
semacs.setStyle("cursor", { foreground: "#202124", background: "#ffcc66" });

semacs.registerCommand("uppercase-region", text => text.toUpperCase());
semacs.registerCommand("lowercase-region", text => text.toLowerCase());

semacs.bindKey("ctrl-c u", "uppercase-region");
semacs.bindKey("ctrl-c U", "uppercase-region");
semacs.bindKey("ctrl-c l", "lowercase-region");
