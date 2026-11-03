(() => {
    let open = true;
    const commands = [];
    const bindings = [];
    let font = {};
    const themes = [];
    let theme = "light";
    const styles = [];
    const object = value => value !== null && typeof value === "object" && !Array.isArray(value);
    const copyStyle = (name, value) => {
        if (!object(value) || Object.keys(value).some(key => key !== "foreground" && key !== "background"))
            throw new TypeError("Style accepts only foreground and background");
        for (const color of Object.values(value))
            if (typeof color !== "string" || !/^#[0-9a-fA-F]{6}$/.test(color))
                throw new TypeError("Color must be #RRGGBB");
        return {name, ...value};
    };
    const check = () => {
        if (!open) throw new Error("Configuration APIs are only available while loading init.js");
    };
    const api = Object.freeze({
        registerCommand(name, run) {
            check();
            if (commands.length >= 256) throw new Error("At most 256 script commands are allowed");
            if (typeof name !== "string" || name.length > 128 || typeof run !== "function")
                throw new TypeError("registerCommand expects a name and a function");
            commands.push({name, run});
        },
        bindKey(keys, command) {
            check();
            if (bindings.length >= 1024) throw new Error("At most 1024 custom key bindings are allowed");
            if (typeof keys !== "string" || keys.length > 256 ||
                (command !== null && (typeof command !== "string" || command.length > 128)))
                throw new TypeError("bindKey expects a key sequence and a command name or null");
            bindings.push({keys, command});
        },
        defineTheme(name, definition) {
            check();
            if (themes.length >= 32) throw new Error("At most 32 custom themes are allowed");
            if (typeof name !== "string" || name.length > 64 || !object(definition) ||
                Object.keys(definition).some(key => key !== "appearance" && key !== "styles") ||
                (definition.appearance !== "light" && definition.appearance !== "dark") ||
                !object(definition.styles) || Object.keys(definition.styles).length > 22)
                throw new TypeError("defineTheme expects a name and {appearance, styles}");
            themes.push({name, appearance: definition.appearance,
                styles: Object.entries(definition.styles).map(([name, value]) => copyStyle(name, value))});
        },
        loadTheme(name) {
            check();
            if (typeof name !== "string" || name.length > 64)
                throw new TypeError("loadTheme expects a theme name");
            theme = name;
        },
        setStyle(name, value) {
            check();
            if (styles.length >= 128) throw new Error("At most 128 style overrides are allowed");
            if (typeof name !== "string" || name.length > 64)
                throw new TypeError("setStyle expects a style name");
            styles.push(copyStyle(name, value));
        },
        setFont(settings) {
            check();
            if (!settings || typeof settings !== "object" ||
                Object.keys(settings).some(key => key !== "family" && key !== "size"))
                throw new TypeError("setFont accepts only family and size");
            if (settings.family !== undefined && typeof settings.family !== "string")
                throw new TypeError("Font family must be a string");
            if (settings.size !== undefined && typeof settings.size !== "number")
                throw new TypeError("Font size must be a number");
            font = {...font, ...settings};
        },
    });
    return {
        api,
        finish() {
            open = false;
            return {commands, bindings, font, themes, theme, styles};
        },
    };
})()
