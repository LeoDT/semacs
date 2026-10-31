# 决策记录

状态：D1–D13 保留原始决策记录；D14 是本次方案评估后的现行修正，优先于其中冲突的旧假设。
方案 B 只获准进入 spike，尚未获证实可满足全部行为要求。新增事实通过追加决策记录，不改写历史。

| ID | 决策 | 状态 |
|---|---|---|
| D1 | 依赖 crates.io 的 gpui-kit 0.8.1，本地源码仅参考 | 已决定 |
| D2 | Linux / Wayland 优先，平台差异收敛到 `platform.rs` | 已决定 |
| D3 | 纯 Emacs 键位，不做 CUA | 已决定 |
| D4 | 自研键位分发器，挂在 `intercept_keystrokes` 上 | 已决定 |
| D5 | 文本底座复用 `EditorState`（方案 B） | 候选路径，受 D14 门禁约束 |
| D6 | 编辑语义归我们，`EditorState` 只做文本 / caret / 排版 / IME | 已决定 |
| D7 | region 用 `RangeDecoration` 自绘，不用 EditorState 的 selection | 保留方向，同步 / marker 假设由 D14 修正 |
| D8 | undo 委托 `EditorState` 的 Undo / Redo action，接受已登记差异 | 分组与完成机制待验证，见 D14 |
| D9 | 每 window 一个 buffer；同 buffer 多窗口不支持，预留 model/view 缝 | v1 split 范围由 D14 取代 |
| D10 | 单 crate，暂不拆 workspace | 已决定 |
| D11 | 命令注册表是 M-x / C-h 的唯一数据源 | 已决定 |
| D12 | 不模仿 Emacs 外观，视觉用 gpui-kit | 已决定 |
| D13 | v1 键位写在 Rust 代码里，用户配置格式待定 | 已决定 |
| D14 | 修正输入 / mark / undo 假设；spike 门禁；v1 截止 M3 | 已决定，API 缺口的处理选项仍待确认 |

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

> 历史记录：下文“公开 API 均可绕过”的判断未经完整验证，已由 D14 撤回；方案 B 先做 spike。

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

> 历史记录：保留自有 point / mark 与 region 高亮方向。下文空 `RangeDecoration` 兜底与简化同步假设不再有效，现行约束见 D14。

**决策**：不把 `EditorState` 的 selection 当 region。point / mark 自己维护；
region 用 `create_range_decorations_collection(RangeDecoration::Fill)` 绘制。

**理由**：公开 API 的 `set_selected_range` 强制 `reversed = false`，caret 永远在 range 末端，
无法表达 Emacs 的"mark 在 point 之后"。`RangeDecoration` 会随编辑自动调整，等价于简化版 overlay。

**后果**：

- 需要 point ↔ cursor 同步与鼠标选择的反向同步；
- mark 需要 marker 语义（见 architecture.md 第 6 节），必要时用空 `RangeDecoration` 跟踪；
- 原生 selection 只在鼠标拖选过程中短暂存在。

## D8 undo

> 历史记录：下文“每条命令后同步即可复现边界”、分组实测等价与清 mark 的推论不再作为实现依据；见 D14 与更新后的验收矩阵。

**决策**：v1 委托 `EditorState` 的 `Undo` / `Redo` action（`window.dispatch_action`）。
绑定与 Emacs 31 一致。接受两个已知差异：连续 yank 不合并、无 undo-in-region。
每条命令结束同步 caret，以复现 Emacs 的 undo 命令边界。

**理由**：实测分组行为与 Emacs 高度接近（见 [对照表](emacs-fidelity.md)）；自建 undo 的收益不足以抵消
"平台自插入 / IME 不经过编辑入口"带来的记录成本。

**后果**：undo 是异步派发；undo 后从 `cursor()` 同步 point 并清 mark。
若将来差异变得刺眼，可请上游公开 `begin_transaction / end_transaction`，或自研 view 时一并重做。

## D9 多窗口

> 历史记录：下文“v1 支持不同 buffer split”的范围由 D14 取代；v1 不含任何 split。

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

## D14 方案评估修正与实施门禁

**决策**：先执行 [S1–S5](plan.md#1-先做-spike不写正式代码)，通过后再进入 M0。
本次修正没有批准新增 F3 差异、依赖 patch、上游 API 修改或自研 view。

**理由 / 证据边界**：

- 本次检查的本地 gpui-kit 参考快照为 `94480398`；依据文件包括
  `crates/base/src/input/base/{state,movement,undo_manager}.rs` 与
  `crates/base/src/input/editor/decorations.rs`。公开 `insert` / `replace` 使用 Atomic 意图，
  原生 Backspace / Delete 使用专用合并意图；`set_selected_range` 会打断合并。
  因此，旧表的底座行为不能证明自有命令入口的 undo 分组一致。
- 空 `RangeDecoration` 会被过滤，不能承担通用 marker 语义。
- IME handler 可接收指定替换范围，旧 point / 新 cursor / 长度差不足以定位一般编辑。
- 本机 Emacs 31.1 的 `C-x a` 为 abbreviation 前缀，不是 undefined 测试基准。
- 以上是参考源码 / 绑定核对，不是 crates.io 依赖构建、完整 Emacs 对照脚本或 Wayland 运行验证结果。

**现行约束**：

1. caret 同步与 undo 分组分开考虑。异步 action 必须在正确目标完成后再收敛 point / mark；
   不以派发返回代表完成，不在每条命令后无条件写回 selection。
2. mark 位置与 region 激活状态独立；deactivate 不清除 mark。
   外部输入、IME 更新 / 提交 / 取消、鼠标与历史回放的来源、替换范围和完成时机须在 S4 验证。
3. 普通 Meta 不自插入，Shift 字符可以输入；AltGr、死键与 IME 显式分类。
   编辑器目标内未绑定命令键统一消费并回显 undefined，不静默 `Ignored`。
4. Emacs 基准固定版本、major mode、选项、初始状态、脚本与 command loop 驱动方式；
   semacs 必须走真实命令链路。旧 undo 对照结论降为待复测项，已登记 F3 不因此扩大。
5. v1 为 M0–M3：单顶层窗口、一个正文 view、可切换多个 buffer；`M-x` / minibuffer 在 M2，
   连续 kill 规则在 M1，性能基线从 M0 / M1 建立，M2 覆盖文件安全。
   所有 split、isearch、`M-y`、`C-u` 都在 v1 之后评估。D9 的 v1 split 承诺由本条取代。

**后果 / 失败门禁**：任何 spike 失败或关键状态无法可靠观测，暂停正式实现，提交最小复现、
API 缺口、受影响的保真度项与候选路径。是否接受新差异、补上游 API、引入依赖 patch 或更换底座，
须确认后另记决策；不得在实现中选择成本最低的降级作为默认行为。

---

## 待决事项

- 阻断性 API 缺口：S3 / S4 的实际可行机制；失败时在新增差异、补上游 API、依赖 patch 或更换底座之间另行确认。
- 文件层：v1 保证 UTF-8 / LF；CRLF / BOM 等输入的拒绝或保留策略须在 M2 明确，禁止静默有损转换。
  自动保存、备份文件属后续增强；确认文案在 M2 固定。
- v1 键位细节：`ESC` / Meta 输入与取消的完整交互、前缀显示、`C-?` 平台键名；前缀默认无超时。
  `RET` / `TAB` / `C-j` 的命令与缩进选项须在 M1 前固定，不沿用未经核对的全局绑定假设。
- kill ring 与系统剪贴板的同步策略；不影响应用内 kill / yank 的 v1 验收。
- 首轮基线测量后的性能预算与样本规模。
- v1 后的配置格式、isearch / occur / syntax highlighting、`M-y`、`C-u` 及更多重复语义的优先级。
- 后续 split 的键位与限制、同 buffer 多视图是否需要自研 view；不作为 v1 前置工作。
