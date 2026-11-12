# 需求与范围

## 产品定位

semacs 是用 Emacs 操作逻辑查看代码的桌面工具：语义按 Emacs，外观不复刻 Emacs。

目标按优先级排列：

1. 按键、命令、前缀、取消与未定义键行为一致。
2. 代码查看流程可用：打开文件 / 项目、浏览目录树、阅读带语法高亮的源码、搜索当前文件 / 项目、查看工作区 / 暂存区 / 提交 diff 与历史。
3. point / mark、region、kill ring、undo 与 minibuffer 的可观察行为一致；保留打开 → 编辑 → 保存 → 切换 buffer → 退出的基础编辑闭环。
4. Rust 核心提供文本编辑与浏览的基础能力，脚本组合扩展功能；扩展不能破坏基础编辑 / 浏览的性能约束。

基础编辑仍是产品能力，但 semacs 不追求成为通用 Emacs 替代品或完整 Magit / Git 客户端。后续编辑增强以自动 / 智能缩进等阅读所需能力为主，主要精力放在查看、导航与 diff。

## 平台与交互边界

- Linux 优先，Wayland 为主要验收环境；X11 的启动和基础输入是兼容目标，不表示已验证。
- 一个 OS 窗口；允许内部 split、多个 buffer 与同 buffer 多 view，不提供多 frame。
- Emacs 键位及已批准的[保真度差异](emacs-fidelity.md#2-保真度分级)，不并存 CUA 编辑模型；窗口管理与输入法切换的保留键由平台处理。
- 中文 / IME 属于基本输入能力，不是后续增强。

## 历史 v1 范围（已完成）

本节记录 v1 当时的交付边界，保留历史语义；后续阶段已经在部分条目之外交付，因此本节不再作为当前方向或发布条件。当前方向见下节。

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

## 当前方向（v1 后）

用户已确认将主要精力从继续复刻 Emacs / Magit 基础功能转为“使用 Emacs 操作逻辑的代码查看工具”。方向包括：

- **文件与项目导航**：文本式文件夹 tree viewer，参考 treemacs：独立文本 buffer / 专用左侧 side window（独立于普通 split）、展开折叠、键盘导航、打开文件；基础 project 能力为项目根识别、项目切换与项目内文件查找。
- **diff 与历史**：以查看为主；在现有 Magit 风格 status / diff / log / revision 上补任意 revision 与 range 查看。已交付的 index / commit 写入保持可用，不继续以扩展 Git 写工作流为主线。
- **代码阅读**：沿用现有 tree-sitter 高亮、分屏、主题、字体、长行与折行能力；搜索是优先增强方向，结构导航、tree 集成等按实际使用推进。
- **基础编辑**：保留已交付的打开、编辑、保存、undo、mark / region、kill / yank 与 minibuffer；自动 / 智能缩进只记录为后续需求，暂不排期。
- **脚本与配置**：继续作为定制入口，按实际功能增量补充 API，不预建完整插件平台。

以上是产品方向，不是已批准的实施阶段；具体范围、交互形态和顺序只在 [实施计划](plan.md) 维护。

## 非目标（F4）

以下是“不做”，而非待排期的 v1 后能力：

- elisp、Emacs 包生态及 Emacs 配置语言兼容。
- 包管理器。
- org-mode、tramp、daemon / client / server。
- 完整 dired 与文件管理器语义（标记、批量重命名、递归文件操作等）；tree viewer 的交互与文件操作范围另行确认。
- 多 frame、tab-bar、GUI 菜单栏与工具栏。
- 终端复用与 TTY 界面。

## 约束入口

- [Emacs 保真度](emacs-fidelity.md)：键位、行为契约与批准差异。
- [技术决策](decisions.md)：编辑器所有权、命令体系与源码复用边界。
- [性能预算](performance.md)：已批准的性能约束及测量边界。
- [实施计划](plan.md)：交付顺序。
