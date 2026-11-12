# semacs

[English](README.md) | [中文](README.zh.md)

semacs is an Emacs-style code viewer with a modern GPU UI, built in Rust on top of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) through [gpui-kit](https://github.com/longbridge/gpui-kit). It keeps Emacs' keyboard and command model — prefixes, minibuffer, buffers, marks, kill ring and undo — while focusing on reading and navigating code. It does not aim to run Emacs Lisp or reproduce the Emacs package ecosystem.

> **Status:** early development. Basic editing and Magit-style Git views are usable, and the project is being refocused on code browsing. Interfaces and behavior may still change.

## Features

- Emacs-style keys, prefixes, `M-x`, minibuffer completion and command history.
- Buffers, views and splits; UTF-8/BOM and LF/CRLF round-tripping.
- Basic editing with undo/redo, mark/region, kill/yank and mouse selection.
- Tree-sitter syntax highlighting for Rust, JavaScript/JSX, TypeScript/TSX, Markdown, HTML and CSS.
- Themes, bundled Lilex fonts and named text styles.
- Magit-style status, staged/unstaged diffs, hunk staging, log/revision views and commit/amend/reword flows.
- JavaScript configuration in `init.js` for fonts, themes, keybindings and commands.

## Roadmap

Current work focuses on code browsing:

- A text-based file tree in a dedicated side window, inspired by treemacs.
- Basic project support: root detection, project switching and project file finding.
- Viewing arbitrary revisions and commit ranges in the existing diff viewer.
- Search, outline navigation and tree integration.

Basic editing remains supported. Later editing work is expected to focus on automatic indentation rather than replicating every Emacs editing command.

## Requirements

- Linux; Wayland is the primary target. X11 startup and basic input are compatibility goals but are not fully verified.
- A recent Rust toolchain with edition 2024 support.
- A GPU/display environment supported by GPUI.

## Build and run

```sh
cargo build --release
cargo run --release -- path/to/file
```

Run without a file to start in `*scratch*`:

```sh
cargo run --release
```

The binary is written to `target/release/semacs`.

## Configuration

semacs reads `$XDG_CONFIG_HOME/semacs/init.js`, falling back to `$HOME/.config/semacs/init.js`. The configuration is a single JavaScript file; see [`examples/init.js`](examples/init.js) and the [configuration guide](docs/scripting.md).

## Documentation

Project documentation is currently written in Chinese; start with the [documentation index](docs/README.md).

- [Requirements and direction](docs/requirements.md)
- [Emacs fidelity contracts](docs/emacs-fidelity.md)
- [Implementation plan](docs/plan.md)
- [JavaScript configuration](docs/scripting.md)
- [Third-party notices](THIRD_PARTY.md)

## Contributing

Bug reports and focused pull requests are welcome. For larger changes, please open an issue first so the scope can be discussed.

## License

semacs is free software under the GNU General Public License, version 3 or later (`GPL-3.0-or-later`). See [LICENSE](LICENSE) and [LICENSE-GPL](LICENSE-GPL). Third-party code and bundled fonts retain their own licenses; see [THIRD_PARTY.md](THIRD_PARTY.md).
