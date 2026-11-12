# semacs

[English](README.md) | [中文](README.zh.md)

semacs 是一个使用 Emacs 操作逻辑的代码查看器，基于 Rust、[gpui-kit](https://github.com/longbridge/gpui-kit) 与 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 构建现代 GPU 桌面界面。它保留 Emacs 的键盘与命令模型——前缀键、minibuffer、buffer、mark、kill ring 与 undo——但重点放在阅读和浏览代码。semacs 不试图运行 Emacs Lisp，也不复刻 Emacs 包生态。

> **状态**：早期开发中。基础编辑与 Magit 风格 Git 视图已可用，项目正在转向代码浏览。接口与行为仍可能变化。

## 功能

- Emacs 风格按键、前缀键、`M-x`、minibuffer 补全与命令历史。
- buffer、view 与 split；UTF-8/BOM、LF/CRLF 文件往返。
- 基础编辑：undo/redo、mark/region、kill/yank 与鼠标选区。
- tree-sitter 语法高亮：Rust、JavaScript/JSX、TypeScript/TSX、Markdown、HTML 与 CSS。
- 主题、随包 Lilex 字体与命名文字样式。
- Magit 风格 status、staged/unstaged diff、单 hunk stage、log/revision 查看及 commit/amend/reword 流程。
- 通过 `init.js` 配置字体、主题、键位与命令。

## 路线图

当前工作聚焦代码浏览：

- 专用 side window 中的文本文件树，参考 treemacs。
- 基础 project 能力：项目根识别、项目切换与项目内文件查找。
- 在现有 diff 查看器中查看任意 revision 与 commit range。
- 搜索、结构导航与文件树集成。

基础编辑继续保留；后续编辑增强预计以自动缩进为主，而不是补齐 Emacs 的全部编辑命令。

## 环境要求

- Linux；主要目标为 Wayland。X11 启动与基础输入是兼容目标，但尚未完整验证。
- 支持 edition 2024 的较新 Rust 工具链。
- GPUI 支持的 GPU / 显示环境。

## 构建与运行

```sh
cargo build --release
cargo run --release -- path/to/file
```

不传文件时从 `*scratch*` 启动：

```sh
cargo run --release
```

二进制输出到 `target/release/semacs`。

## 配置

semacs 读取 `$XDG_CONFIG_HOME/semacs/init.js`；变量不可用时回退到 `$HOME/.config/semacs/init.js`。配置是一个普通 JavaScript 文件；示例见 [`examples/init.js`](examples/init.js)，说明见 [配置文档](docs/scripting.md)。

## 文档

项目文档目前均为中文，从 [文档索引](docs/README.md) 开始。

- [需求与方向](docs/requirements.md)
- [Emacs 保真度契约](docs/emacs-fidelity.md)
- [实施计划](docs/plan.md)
- [JS 配置](docs/scripting.md)
- [第三方来源与许可](THIRD_PARTY.md)

## 参与

欢迎提交缺陷报告和范围明确的 pull request；较大的改动请先开 issue 讨论范围。

## 许可

semacs 以 GNU General Public License version 3 or later（`GPL-3.0-or-later`）发布。见 [LICENSE](LICENSE) 与 [LICENSE-GPL](LICENSE-GPL)。第三方代码与随包字体保留各自许可；见 [THIRD_PARTY.md](THIRD_PARTY.md)。
