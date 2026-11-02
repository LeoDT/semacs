(() => {
    let open = true;
    const commands = [];
    const bindings = [];
    let font = {};
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
            return {commands, bindings, font};
        },
    };
})()
