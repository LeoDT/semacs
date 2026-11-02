/// <reference path="../types/semacs.d.ts" />

semacs.setFont({ family: "monospace", size: 18 });

semacs.registerCommand("uppercase-region", text => text.toUpperCase());
semacs.registerCommand("lowercase-region", text => text.toLowerCase());

semacs.bindKey("ctrl-c u", "uppercase-region");
semacs.bindKey("ctrl-c U", "uppercase-region");
semacs.bindKey("ctrl-c l", "lowercase-region");
