# 决策记录

状态：D1–D13 已决定。实现阶段如发现新事实，追加新决策而不是改写历史。

| ID | 决策 | 状态 |
|---|---|---|
| D1 | 依赖 crates.io 的 gpui-kit 0.8.1，本地源码仅参考 | 已决定 |
| D2 | Linux / Wayland 优先，平台差异收敛到 `platform.rs` | 已决定 |
| D3 | 纯 Emacs 键位，不做 CUA | 已决定 |
| D4 | 自研键位分发器，挂在 `intercept_keystrokes` 上 | 已决定 |
| D5 | 文本底座复用 `EditorState`（方案 B） | 已决定 |
| D6 | 编辑语义归我们，`EditorState` 只做文本 / caret / 排版 / IME | 已决定 |
| D7 | region 用 `RangeDecoration` 自绘，不用 EditorState 的 selection | 已决定 |
| D8 | undo 委托 `EditorState` 的 Undo / Redo action，接受已登记差异 | 已决定 |
| D9 | v1 每 window 一个 buffer；同 buffer 多窗口不支持，预留 model/view 缝 | 已决定 |
| D10 | 单 crate，暂不拆 workspace | 已决定 |
| D11 | 命令注册表是 M-x / C-h 的唯一数据源 | 已决定 |
| D12 | 不模仿 Emacs 外观，视觉用 gpui-kit | 已决定 |
| D13 | v1 键位写在 Rust 代码里，用户配置格式待定 | 已决定 |

---

## D1 依赖方式

**决策**：`gpui-kit = "0.8.1"` 来自 crates.io，不写 path 依赖。
本地 `~/ghq/github.com/longbridge/gpui-kit` 只用于阅读实现。

**理由**：构建可复现、升级路径清晰；不把本地仓库状态引入构建。

**被否**：path 依赖（方便补 API，但引入同步成本与不可复现构建）。

**后果**：遇到公开 API 缺口时优先用现有 API 绕过；确实需要时再单独决定是否提上游 PR 或临时 patch。

## D2 平台

**决策**：Linux 优先，Wayland 为验收环境；X11 保持可运行。所有平台差异收敛在 `src/platform.rs`。

**理由**：gpui-kit / GPUI 已经处理了大部分窗口、输入法、字体适配；我们只需要处理键名与自插入判定等薄层。

**后果**：macOS / Windows 适配时只改 platform 模块和少量 modifier 命名，不散落到命令层。

## D3 纯 Emacs 键位

**决策**：`C-c` / `C-x` 是前缀；复制 / 粘贴走 `M-w` / `C-y`；`C-v` 是翻页。
不做 `Ctrl-C / Ctrl-V` 的 CUA 行为。仅保留窗口管理层面的系统键。

**理由**：项目重点是 Emacs 操作逻辑；同时保留两套键位会破坏前缀语义和可预测性。

**后果**：系统剪贴板与 kill ring 的同步策略待定（见待决事项）。

## D4 键位分发器

**决策**：自己实现 keymap / 前缀 / 命令解析，通过 `App::intercept_keystrokes` 在
GPUI keymap 之前接管按键；`stop_propagation()` 消费事件。

**理由**：

- GPUI keymap 的 action 是静态类型，`M-x` / `describe-key` / transient map 都需要命令名注册表；
- GPUI 的 pending input 有超时，Emacs 的前缀默认无超时；
- `C-g` 取消、未定义键回显、模式化 keymap 用自有引擎更直接；
- interceptor 同时能阻止平台把字符插入 buffer，正是"特殊模式下字母键是命令"所必需的。

**被否**：把 Emacs 键位全部注册成 GPUI 的 `KeyBinding` + action。快速，但语义受限于框架，
且无法自然表达 `M-x` 与动态 keymap。

**后果**：键位引擎要自己写并测试；换来的是一套可复用于 magit transient 的通用机制。

## D5 文本底座

**决策**：复用 gpui-kit 的 `EditorState`（`InputBaseState<EditorMode>`）作为文本 + caret +
排版 + IME + 滚动 + 装饰的底座，不自研文本引擎。

**理由**：它已经覆盖了最难写的部分：Rope 编辑、软换行、虚拟滚动、鼠标命中测试、IME / 预编辑、
剪贴板、无障碍、undo 事务、`RangeDecoration`。自研这些是数周且长期消耗边角案例。

**被否**：自研（方案 C）。控制力最强，但 v1 目标是尽快得到可用的 Emacs 交互骨架。

**后果**：

- 同一 buffer 只能有一个 caret / 滚动位置（D9）；
- undo 分组由它的实现决定（D8）；
- point / mark / region 需要我们自建（D6 / D7）；
- 公开 API 缺口需绕过（当前已确认都可以绕过）。

## D6 语义归属

**决策**：Emacs 语义层完全归我们：point / mark / region、motion、kill ring、命令、键位、
minibuffer、undo 边界。`EditorState` 只负责"文本 + 画 caret + 排版 + 输入"。

**理由**：项目重点是模仿 Emacs 的操作逻辑；把 motion / 选区语义交给 Zed 风格的编辑器，
保真度会被框架的上限限制。

**后果**：所有运动目标位置由我们用 Rope 计算，不派发 GPUI 的 `Move*` / `Select*` action。

## D7 region 与 mark

**决策**：不把 `EditorState` 的 selection 当 region。point / mark 自己维护；
region 用 `create_range_decorations_collection(RangeDecoration::Fill)` 绘制。

**理由**：公开 API 的 `set_selected_range` 强制 `reversed = false`，caret 永远在 range 末端，
无法表达 Emacs 的"mark 在 point 之后"。`RangeDecoration` 会随编辑自动调整，等价于简化版 overlay。

**后果**：

- 需要 point ↔ cursor 同步与鼠标选择的反向同步；
- mark 需要 marker 语义（见 architecture.md 第 6 节），必要时用空 `RangeDecoration` 跟踪；
- 原生 selection 只在鼠标拖选过程中短暂存在。

## D8 undo

**决策**：v1 委托 `EditorState` 的 `Undo` / `Redo` action（`window.dispatch_action`）。
绑定与 Emacs 31 一致。接受两个已知差异：连续 yank 不合并、无 undo-in-region。
每条命令结束同步 caret，以复现 Emacs 的 undo 命令边界。

**理由**：实测分组行为与 Emacs 高度接近（见 [对照表](emacs-fidelity.md)）；自建 undo 的收益不足以抵消
"平台自插入 / IME 不经过编辑入口"带来的记录成本。

**后果**：undo 是异步派发；undo 后从 `cursor()` 同步 point 并清 mark。
若将来差异变得刺眼，可请上游公开 `begin_transaction / end_transaction`，或自研 view 时一并重做。

## D9 多窗口

**决策**：v1 每个 window 一个 buffer；支持不同 buffer 的 split 展示；
同一 buffer 同时显示在两个 window 不支持，UI 明确提示而不是静默共享 point。

**理由**：`EditorState` 把 text / caret / scroll 绑在一个 state 上；要真正拆分需要重写 view（D5 后果）。
不同 buffer 的 split 已经覆盖大部分 Emacs 使用场景。

**后果**：`C-x 2/3/o` 放到 M4；同 buffer 双窗口在 M4 评估是否启动自研 view。
`Buffer` / `BufferView` 的窄接口保证上层不受影响。

## D10 工程结构

**决策**：单 crate `semacs`，模块内部分层；不提前拆 workspace。

**理由**：KISS；当前没有独立发布的库。编译时间成为问题时再拆。

## D11 命令注册表

**决策**：`CommandSpec { name, doc, execute }` 静态注册；`M-x`、将来的 `C-h k` / `C-h f`
共用这一份数据。命令名使用 Emacs 名称。

**理由**：避免键位表与命令实现两份真相；也让 echo / 帮助可以显示真实命令名。

## D12 外观

**决策**：不模仿 Emacs 外观；使用 gpui-kit 的主题、字体、滚动、动画、菜单等原生能力。

**理由**：项目约束是"操作与按键逻辑"，不是视觉复刻；外观复刻会消耗大量时间且收益低。

## D13 键位配置

**决策**：v1 默认键位写在 Rust 的 `keys/emacs.rs`；用户自定义键位文件与配置格式延后。

**理由**：先验证键位引擎与命令覆盖；配置格式需要配合 `describe-key` / 用户覆盖优先级一起设计。

---

## 待决事项

- 配置系统：Rust 构建期、TOML、还是（不做的）elisp 子集。
- 文件层：编码（UTF-8 / CRLF / BOM）、自动保存、备份文件、`C-x C-c` 的确认文案。
- 键位细节：`ESC` / `M-` / `C-g` / `C-u` / 前缀显示 / `C-?` 平台键名。
- kill ring 与系统剪贴板的同步。
- 连续 yank 的 undo 合并是否需要上游 `begin_transaction`。
- 同 buffer 多窗口是否启动方案 2（自研 view）。
- isearch / occur / syntax highlighting 的优先级。
