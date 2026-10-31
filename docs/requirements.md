# 需求与范围

## 1. 项目定位

semacs 不是"另一个代码编辑器"，而是**把 Emacs 的操作模型搬到现代 GPU UI 上**：

- 输入方式是 Emacs 的键位体系：前缀键、`C-g`、`M-x`、kill ring、mark/region、minibuffer；
- 数据模型是 Emacs 的 buffer / window / mode / command；
- 视觉与交互部件使用 gpui-kit 的原生实现，不模拟 Emacs 的外观。

后续阶段在同一套框架上增加 diff 查看器与 magit 风格的 git 界面。

## 2. 目标（按优先级）

1. **按键与命令语义一致**（最高优先级）。
   同一个键序列触发同语义的命令；前缀、未定义键、取消、自插入的行为与 Emacs 一致。
2. **可观察的编辑行为一致**。
   point / mark、region、kill ring、motion、undo 的命令边界、minibuffer 读写流程以 Emacs 为参照。
3. **日常文本编辑可用**。
   打开 / 保存 / 切换 buffer、基本 motion 与编辑、纯键盘操作。
4. **为 diff / magit 留好结构**。
   模式化 keymap、只读 buffer、分层 / transient 键位表、区域高亮与 section 折叠。

## 3. 核心原则

- **键盘优先**：v1 的所有基础编辑必须能纯键盘完成，不依赖菜单或鼠标。
- **语义按 Emacs，外观按 GPUI**：不模拟 Emacs 的 GTK / 终端外观、字体、菜单栏、工具栏、滚动条样式。
- **不做 elisp**：命令用 Rust 静态注册，`M-x` 从命令注册表读取；不实现 elisp 解释器或包生态。
- **单进程、单顶层窗口**：v1 只有一个 OS 窗口；Emacs 的 window split 见非目标与后续阶段。
- **不留技术债**：键位 / 命令 / buffer 三层之间用窄接口隔离，未来替换文本引擎只影响 buffer 层。
- **KISS / YAGNI**：未在 [Emacs 保真度](emacs-fidelity.md) 列为 F1 / F2 的特性不提前实现。

## 4. v1 范围

包含：

- 启动时打开命令行参数指定的文件，或创建 scratch buffer；
- 单顶层窗口、一个正文 view，管理并切换多个 buffer；mode line 与 echo area；
- Emacs 基本 motion / 编辑 / kill ring / mark-region / undo；
- `M-x`、`C-x C-f`、`C-x C-s`、`C-x C-w`、`C-x b`、`C-x k`、`C-x C-c`；
- 文件读写（UTF-8 有 / 无 BOM，LF / CRLF 无损保留），dirty 标记与退出确认；
- 键位引擎、命令注册表、major mode 框架；
- 自动化测试：键位解析、文本变换、真实按键交互。

不包含（v1）：

- isearch、正则替换、rectangle、键盘宏、narrowing、间接 buffer；
- 所有 window split（包括不同 buffer）、多 frame、tab-bar；
- 用户配置系统与自定义键位文件；
- 语法高亮、LSP、正文代码补全（minibuffer 的命令名 / 路径 / buffer 名基础补全包含在 v1）；
- 插件 / 脚本扩展；
- diff 与 git（下一阶段）。

v1 对应实施计划 M0–M3，前置 S1–S5 必须通过；M4 起为后续阶段。
`C-u`、`M-y`、isearch 与 split 不作为 v1 发布条件，不因出现在保真度分级中自动纳入范围。

## 5. 长期非目标

- 兼容 elisp 或 Emacs 包；
- 复刻 org-mode、dired、tramp、daemon / client / server；
- 复刻 Emacs 的 GUI 控件（菜单、工具栏、滚动条样式）；
- 在功能列表上追求 "Emacs 有的都要有"。

## 6. 技术约束

- Rust 2024 edition，单 crate（`semacs`）。
- 依赖 crates.io 的 `gpui-kit = "0.8.1"`；本地 gpui-kit 源码只作阅读参考，不作为 path 依赖。
- 平台：Linux 优先，Wayland 为验收环境，X11 保持可用；平台差异收敛在 `src/platform.rs`。
- 键位：纯 Emacs。不做 CUA 的 `Ctrl-C / Ctrl-V`；复制 / 粘贴走 `M-w` / `C-y`。仅保留窗口管理层面的系统键。
- 文本引擎：复用 gpui-kit 的 `EditorState` 作为文本 + caret + 排版 + IME 底座；编辑语义自己实现。
  undo 分组、外部编辑观测与 marker 跟随的机制已通过 spike；正式实现仍按里程碑完成产品回归。
  若实施中发现新的 API 缺口，暂停并按 [D14](decisions.md#d14-方案评估修正与实施门禁) 确认新增差异或 API / 依赖调整，不能隐式降低 F1。

## 7. 验收标准（v1）

- 不碰鼠标可以完成：打开文件 → 编辑 → 保存 → 切换 buffer → 退出；每个操作都有 Emacs 键位。
- `C-x` / `M-` 前缀与 `C-g` 取消行为正确；未定义键不插入字符，并在 echo area 报错。
- 中文 / IME 输入正常；mark / region 与 kill ring 行为符合 [保真度清单](emacs-fidelity.md)。
- 预编辑期间命令与前缀按 D17 拒绝并提示，组合保持原样；关闭 / 失焦按 D19 取消未提交预编辑后继续处理，不改变原有 modified 状态，关闭仍须执行未保存确认；不保存临时文本，不把迟到回调投递到新目标；pending 前缀不可被直接文本输入绕过。
- A → B → A 保留各 buffer 的 point / mark，激活状态按命令语义变化；正文与 minibuffer 共用基础编辑语义及 kill ring，undo 历史独立，prompt 不可编辑。
- 关键行为有测试：键序列解析、命令执行后的 point / mark / 激活状态 / 文本、undo 分组、退出脏检查。
- 按 D20 无损读写 UTF-8 有 / 无 BOM、统一 LF / CRLF 的文件，保存保留原格式；无效 UTF-8 与未支持编码明确报错，不修改原文件。
- 保存失败不得清 dirty 或破坏原文件；保存 / undo 的 modified 状态遵循 D16 与固定 Emacs 对照，不以磁盘内容比较替代；kill dirty buffer、窗口关闭与取消退出均有验收。
- 已记录大文件打开、滚动与按键延迟的可复现基线；不得等到引入 split 才检查底座性能。

## 8. 后续阶段

1. **diff 查看器**：复用 gpui-kit 的 `component::diff`（参考 `examples/tig`），只读文本视图 + 自己的 mode keymap。
2. **magit 风格 git**：只读 buffer + section 折叠 + transient keymap（例如 `?` 帮助、`s` stage、`c` commit）。
3. 可选增强：isearch / occur、语法高亮、用户键位配置、多窗口与同 buffer 多视图。
