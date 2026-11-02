interface SemacsFontSettings {
    family?: string;
    size?: number;
}

interface SemacsConfigurationApi {
    setFont(settings: SemacsFontSettings): void;
    registerCommand(name: string, transform: (text: string) => string): void;
    bindKey(keys: string, commandName: string | null): void;
}

declare const semacs: Readonly<SemacsConfigurationApi>;
