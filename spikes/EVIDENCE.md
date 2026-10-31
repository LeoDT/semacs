# spike 证据（过程记录；产品级结论以 docs/ 为准）

可执行工程见同目录 `lab/`（headless sway + wtype 场景）与 `reference/`（Emacs 31.1 pty 对照），
用法与坑见 [README.md](README.md)。`lab/cases/*.expected` 是这些结论的落地断言。

环境：gpui-kit 0.8.1（crates.io，Cargo.lock 见本目录）、gpui-pre-* 0.3.10、sway 1.12 headless（WLR_BACKENDS=headless, WLR_RENDERER=pixman）、wtype、
libwayland 1.26、mesa 26.2.5（沙箱无 /dev/dri，故 LIBGL_ALWAYS_SOFTWARE=1 → llvmpipe）、Emacs 31.1（基准导出）。
驱动方式：`scripts/run.sh <case>` 启动私有 sway → 跑 lab → wtype 注入 → 收集日志 / 截图。

## S1（2026-xx，layer 2 实测）
1. `intercept_keystrokes` 收到 `KeystrokeEvent { keystroke, input_preference }`；`cx.stop_propagation()` 后：
   底座 action 不派发、不插字符、元素的 on_key_down 不触发。用例 `s1-consume`：
   ctrl-a（底座 SelectAll/MoveHome）、ctrl-x（Cut）、ctrl-c、ctrl-v、tab（IndentInline）、enter、escape 全部被消费后，
   `text="abc" cursor=3 sel=3..3` 不变，随后自有插入落在 3 → point 未被底座移动。
2. 对照 `s1-log`（不消费）：tab 插入 "  "（`text="  abc"`），enter 插入换行并派发 `InputEvent::PressEnter`。
   → 拦截确实是"tab/enter 不改文本"的原因，而不是底座本来就不动作。
3. 普通字符只来一次 KeyDown（`key="a" key_char=Some("a") pref=keys`），随后 CHANGE 插入。未观察到额外的 CommitString 合成事件。
   源码依据：gpui-pre-linux `CommitString.len()==1` 被转成 KeyDownEvent；多字符走 `ImeInput::InsertText`（绕过键位引擎，S4 处理）。

## S3 预检
4. `Undo` 经 `window.dispatch_action` 返回后文本尚未恢复，Change 通知在之后才到：
   `UNDO after text="abc"` → 随后 `CHANGE text="abcz"`。→ "派发返回 ≠ 完成"，point/mark 收敛必须放在完成观测之后。
5. 自有 `set_selected_range + replace("")/(z)` 的插入与删除各成独立 undo 单元（两次 undo 分别撤销 delete、insert）。
   `set_selected_range` 未把两次编辑合并。

## S3 undo 分组（关键阻断项，2026，layer 2 + Emacs 基准对照）

Emacs 基准（.pi/tmp/emacs-baseline/RESULTS.md，真实 command loop）：连续自插入、连续 DEL、连续 C-d
各自合并成一个 undo 单元；移动 / RET 打断合并；连续 yank 在 Emacs 里也是分开的单元；undo 后输入是新单元。

spike 实测（同一分组规则对照）：

| 路径 | 连续输入 | 连续删除 |
|---|---|---|
| 平台按键路径（自插入，我们不消费） | **合并**（abc → 一次 undo 清空）| — |
| 平台 Backspace action（我们消费 DEL 后派发 `input::Backspace`）| — | **不合并**（3 次 DEL → 每次 undo 恢复 1 字符）|
| 自有 `replace()`（Atomic 意图）| 不合并 | 不合并 |
| 自有 `EntityInputHandler::replace_text_in_range`（无 pending intent，自动 Typing）| 不合并 | 不合并 |

代码依据：`UndoManager::push_batch` 的 coalescing 需要 `!coalescing_boundary && intent != Atomic && 上一个 transaction 同 intent && 批次相邻`；
而 `set_selected_range` / move 系列会调用 `break_transaction_coalescing()`。我们的命令必须同步 point → caret，
因此任何经公开 API 的编辑都会打断合并。`EditIntent` 是 `pub(crate)`，公开编辑入口（`insert`/`replace`/`set_value`/`replace_all`）
全部强制 `Atomic`，没有公开的 intent / transaction 控制。

→ 结论：**用公开 API 无法复现 Emacs 的"连续删除合并"**；只有平台自身的按键输入会合并（那是我们让出去的路径）。

其他 Undo 观察：
- undo 后 point：Emacs redo 把 point 放在被恢复区的起点（point=1），底座 redo 放在末尾（point=3）。
- undo 后底座会留下非空 selection（例如 sel=0..1），不能当作 region，需要按我们自己的 mark 模型归一。
- 空历史连续 undo：底座静默不动作（无 Change），Emacs 只在 echo area 提示；echo 需我们自己做。

## 其他事实
6. 单独按修饰键会合成一个 keystroke（`key="control"`）进入 interceptor；键位引擎需分类处理。
7. gpui-base 的输入状态没有 dirty / 保存点概念（无 `is_modified`）→ dirty 必须由 Buffer 自己维护。
8. gpui-base 默认在 Input context 绑定 ctrl-a/c/x/v/e、tab、enter、escape 等，不拦截会直接改动文本或选区。

## harness 注意事项（会重复踩）
- wtype 每次调用都创建/销毁虚拟键盘；创建后需 `-s 200` 再发键，否则首个 press 被路由到旧焦点而丢失。
- 由此产生的 `leave/capabilities(0)` 会造成日志中 `WINDOW active`/`EVENT Focus|Blur` 抖动，属 harness 噪声，断言时过滤。
- 沙箱没有 /dev/dri：必须 `LIBGL_ALWAYS_SOFTWARE=1`，否则 EGL/ZINK 初始化失败。

## S4 文本输入观测（layer 2 实测，2026）

装载 `ElementInputHandler::new(bounds, our_target_entity)`（`Window::handle_input`，公开 API）之后，
所有平台文本输入都经过我们的适配器，可以拿到明确的范围与内容，不需要事后 diff：

```
KEY   key="a" key_char=Some("a") ...
TARGET replace_text_in_range range_utf16=None text="a" bytes=0..0
CHANGE text="a" ...
```

结论：
1. **平台按键自插入也走 handler**（不是直接改 EditorState），所以适配器能覆盖「平台输入」这一路。
2. `range=None` 表示"当前选择"，由我们的 EditTarget 用自己的 point/region 解析，不必依赖底座 selection。
3. 预编辑与提交可区分：`replace_and_mark_text_in_range`（预编辑）/ `unmark_text` / `replace_text_in_range`（提交）是三个不同调用，
   `marked_text_range` 可读。预编辑文本会临时进入底座文本，因此保存等操作必须先经输入完成屏障。
4. `accepts_text_input` 会被高频轮询；返回 false 只关掉"平台文本输入"这一路，
   按键自插入由 interceptor 单独把关（两道闸门互相独立）。
5. pending 前缀期间到达的字符被 interceptor 判为前缀解析并消费，不插字符（`PREFIX-CHAR ... not inserted`）。

证据边界：预编辑序列由我们自己的适配器直接驱动，不是 Wayland IME 驱动；真实 IME 需在 layer 3 验证。

## 自建 undo 可行性（已获批准的方案，layer 2 实测）

实现：适配器记录每次变更 `{range, old, new, kind}`；命令层用 `with_command_kind` 声明意图
（Typing / DeleteBackward / DeleteForward / Other）；分组规则 = 连续同类合并、非编辑命令插入边界、
undo/redo 不与输入合并；replay 走同一个适配器（`replaying` 标志避免自我记录）。

与 Emacs 基准逐条对照（同一台机器，同一命令顺序）：

| 用例 | Emacs | 自建 undo |
|---|---|---|
| 输入 abc → undo | 全部撤销（1 组） | 全部撤销（1 组）✅ |
| 输入 abc → 移动 → 输入 def → undo | 只撤销 def | 只撤销 def ✅ |
| 连续 DEL ×3 → undo | 三个字符一起回来 | 三个字符一起回来 ✅ |
| 连续自有插入 ×2 → undo | 一组 | 一组 ✅ |
| undo 后 redo | 文本恢复 | 文本恢复 ✅ |

待补：undo 后 point 应回到"该组开始前的 point"（Emacs：undo-del point=4、undo-cd point=1），
当前实现放在组内第一个变更处，需把 `point_before` 存在组上。

已知代价：replay 会写进底座自己的 undo 历史（我们不再派发底座 Undo/Redo，无副作用）；
底座也不再需要承担 dirty / 保存点，全部由我们的 change 记录派生。

## S2 前缀 / 取消 / undefined / minibuffer（layer 2 实测）

键位引擎（`src/keys.rs`，带单测）：`C-` / `M-` / `S-` / `s-` 解析、前缀无超时、
未绑定 + 空前缀 + 可打印 → SelfInsert、有前缀或不可输入 → Undefined。

实测（`scripts/cases/s2-*.sh`）：

| 行为 | 结果 |
|---|---|
| `C-x` | `RESOLVE -> Prefix`，echo 显示 `C-x`，pending 保持 |
| `C-x C-s` | `Execute("save-buffer")`；`C-x u` 同样命中三级序列 |
| `C-x M-a` | `C-x M-a is undefined`，不插字符；随后字符正常插入（Emacs 基准 echo 完全一致）|
| `C-x C-g` | pending 被清除、不插字符，区域停用（见下）|
| 最小 minibuffer（`InputState` + 第二个 EditTarget）| `C-x b` 进入、prompt 显示在 echo、输入走 `TARGET[mini]`、`C-g` 取消并恢复焦点、`RET` 确认；正文文本不受影响 |
| minibuffer 的 motion / 删除 / yank | 与正文同一批命令实现，路由到 mini target（`C-a` 在 minibuffer 内生效）|
| kill ring 共享 | 正文 `C-k` → minibuffer `C-y` 得到同一文本；minibuffer `C-k` 后再在正文 `C-y` 得到新内容 |
| 连续 kill 追加 | 非 kill 命令（如 `yank`）打断追加链（Emacs 规则），ring 保留两条 |

## C-g 与区域（待补矩阵）

- Emacs：`C-x C-g` 在**有活动 region** 时 echo `Quit`、`mark` 位置保留（12→6 不变）、`active` 变 false；
  在**无活动 region** 时 echo `C-x C-g is undefined`。机制一致（prefix 放弃、不插字符），差在 echo 文案。
- 需要单独矩阵：有 / 无活动 region、顶层 `C-g`、minibuffer 内 `C-g`、连续 `C-g`。
- 之前两条互相矛盾的 C-g mark 观测已定位为采集噪声（`append-to-file` 污染 echo area + scratch 缓冲启动噪声），
  基准脚本已改为 `write-region` + `inhibit-message`，并记录 echo 文案。

## S3 矩阵（layer 2 + Emacs 基准逐条对照）

基准脚本已修掉两处采集噪声（`append-to-file` 污染 echo、启动时 scratch 缓冲多一条命令），
并且 setup 后清空 undo 历史、`goto-char point-max`，两侧初始状态一致（Emacs point 为 1-based）。

| 用例 | Emacs 31.1 | semacs spike | 结论 |
|---|---|---|---|
| 输入 abc → C-/ | 全部撤销，1 组 | 同 | ✅ |
| 输入 abc → C-f → 输入 def → C-/ ×2 | 先撤 def，再撤 abc | 同 | ✅ |
| 连续 DEL ×3 → C-/ | 三字符一起回来 | 同（自建 undo）| ✅ |
| `ab\ncd`、C-a → C-k → C-k → C-/ ×2 | 第一次 kill 撤掉，第二次在 EOB 报 `End of buffer` 且不建组；再 C-/ 报 `No further undo information` | 同 | ✅ |
| `ab`、C-a → C-k → C-y → C-/ ×2 | yank / kill 各自一组；yank 把 mark 设在插入起点并回显 `Mark set` | 同 | ✅ |
| 输入 abc → 保存 → 输入 d → C-/ | 文本回 abc，且 **modified 变回 nil** | dirty=false | ✅ |
| 保存 abc → C-/（退到保存点之前）→ 输入 x | 撤销越过保存点时 modified 仍为 nil；再输入变 t | 同（保存点标记被撤销丢弃 → 视为干净）| ✅ |
| 空历史 C-/ | echo `No further undo information` | 同 | ✅ |
| undo 后 point | 回到组开始前的 point（undo-del 4 / undo-cd 1）| `point_before`/`point_after` 已按组记录 | ✅ |

只读目标：输入字符 → echo `Buffer is read-only`，文本不变，`C-a` 等 motion 仍然工作（与 Emacs 一致）；
平台文本输入由 `accepts_text_input=false` 单独拦住。

## 仍然缺的（S4）

1. 鼠标拖选 → point / mark / active 的导入：layer 2 没有指针设备，`wtype` 不能移动指针。
   需要 GPUI 的 `VisualTestContext::simulate_mouse_*`（进程内测试）或 layer 3 手工验证。
2. 组合输入屏障：能观测到预编辑（`replace_and_mark_text_in_range` + `marked_text_range`），
   但**没有公开 API 让应用主动 commit** 平台 IME 的组合输入（只能等平台发提交）。
   这直接影响 D15 要求的"预编辑中保存 / 切换 / 关闭前先收敛输入"，需按 D14 作为 API 缺口单独确认。
