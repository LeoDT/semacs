# 需求与范围

## 产品定位

semacs 将 Emacs 的操作模型带到现代 GPU UI：语义按 Emacs，外观不复刻 Emacs。

目标按优先级排列：

1. 按键、命令、前缀、取消与未定义键行为一致。
2. point / mark、region、kill ring、undo 与 minibuffer 的可观察行为一致。
3. 日常文本编辑可纯键盘完成：打开 → 编辑 → 保存 → 切换 buffer → 退出。
4. Rust 核心提供文本编辑与浏览的基础能力，脚本组合扩展功能；扩展不能破坏基础编辑 / 浏览的性能约束。

## 平台与交互边界

- Linux 优先，Wayland 为主要验收环境；X11 的启动和基础输入是兼容目标，不表示已验证。
- v1 一个 OS 窗口、一个正文 view，允许切换多个 buffer；不提供 split 或同 buffer 多 view。
- 纯 Emacs 键位，不并存 CUA 编辑模型；仅保留窗口管理层面的系统键。
- 中文 / IME 属于基本输入能力，不是后续增强。

## v1 范围

包含：

- 启动时打开命令行参数指定的文件，或创建 scratch buffer。
- mode line、echo area，基本 motion、编辑、kill / yank、mark / region 与 undo / redo。
- `M-x`、打开 / 保存 / 另存文件、切换 / 关闭 buffer 和退出。
- minibuffer 的命令名、路径、buffer 名基础补全。
- UTF-8 有 / 无 BOM、LF / CRLF 文件往返，dirty 标记及数据丢失确认。
- major mode 与分层 keymap 的基础能力。

不包含：

- isearch、正则替换、rectangle、键盘宏、narrowing、间接 buffer。
- `M-y`、`C-u`、mark ring、`C-x C-x`。
- split。
- 用户配置系统、自定义键位文件、插件 / 脚本扩展。
- 语法高亮、LSP、正文代码补全。
- diff 与 git。

上述延后能力不构成 v1 发布条件；保真度分级不自动扩大范围。后续增强按已确认的实施阶段推进，不追溯扩大 v1 验收范围。

## v1 后演进方向

- 自定义字体、字号及字号命令，方块光标。
- Vertico 式 minibuffer：无输入时展示候选，支持纵向选择，复用于命令、文件和 buffer 读入；具体按键与确认行为在对应阶段另行确认。
- JavaScript 同时作为用户配置与扩展语言；自定义键位使用同一配置入口，不并行建设另一套静态配置系统。
- split 与同 buffer 多 view。
- tree-sitter 整合，先提供增量解析与语法高亮，再按实际需求扩展语言能力。
- diff 查看器与 magit 风格 git 界面。

以上是后续方向，不表示已经实现或拥有完整设计；交付顺序只在 [实施计划](plan.md#后续阶段) 维护。其他 v1 外能力仍须另行定范围，不因脚本扩展自动纳入。

## 非目标（F4）

以下是“不做”，而非待排期的 v1 后能力：

- elisp、Emacs 包生态及 Emacs 配置语言兼容。
- 包管理器。
- org-mode、dired、tramp、daemon / client / server。
- 多 frame、tab-bar、GUI 菜单栏与工具栏。
- 终端复用与 TTY 界面。

## 约束入口

- [Emacs 保真度](emacs-fidelity.md)：键位、行为契约与批准差异。
- [技术决策](decisions.md)：编辑器所有权、命令体系与源码复用边界。
- [性能预算](performance.md)：已批准的性能约束及测量边界。
- [实施计划](plan.md)：交付顺序。
