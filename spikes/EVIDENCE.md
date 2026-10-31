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

undo 后 point：已按组记录 `point_before` / `point_after`，与 Emacs（undo-del point=4、undo-cd point=1）一致。
undo 后立即输入：开新组并清空 redo，见 `cases/s3-undo-then-input.*`。

遗留：mark 目前是普通 offset，编辑 / undo 缩短文本后不会跟随移动；Emacs 的 mark 是 marker。
这是 M1 必须补的（D7 的 marker / RangeDecoration 方案），spike 未伪装成已通过。

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

## C-g 与前缀（已定论，修正旧结论）

旧记录“有活动 region 时 `C-x C-g` 回显 `Quit`、无 region 时 `undefined`”是**采集假象**：
`emacs -nw` 的 pty 如果一次 read 里同时拿到 `C-x` 和 `C-g`，Emacs 会当作 quit（`Quit` 并 deactivate）；
分两次 read（真实按键间隔）则得到 `C-x C-g is undefined`，mark 位置与 active 都**不变**。

- 真实 PGTK GUI（Hyprland 会话，真实按键）复核：`C-x C-g is undefined`。
- 顶层 `C-g` 始终 `Quit`（连续按也回显）；minibuffer 中 `C-g` 取消并回显 `Quit`。
- 因此 semacs 去掉了“按 region 分支”的特殊处理：pending 时 `C-g` 走普通 undefined 解析；
  `cases/s2-quit.*` 断言 echo、mark 与 active 不变。

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

## S5 真实键位分类（full-keymap 注入，2026）

`wtype` 上传的 keymap 只有 level 1，Shift / dead key / AltGr 不可信。新增
`scripts/vkbd/`：编译与合成器相同的 evdev keymap（规则 `evdev`、`pc105`、`us` / `us(intl)`），
用命名 keysym 发键。`cases/s5-keymap.*` 与 `real-cases/s5-real-keymap.*`（真实 Hyprland）结果一致：

| 输入 | GPUI keystroke | 解析 | 结论 |
|---|---|---|---|
| Shift+a | `key="a" key_char="A" shift=true` | `S-a` → SelfInsert | Shift 字符正常输入 |
| Shift+1 | `key="!" key_char="!" shift=false` | SelfInsert | 符号不自带 shift |
| Ctrl+Shift+/ | `key="?" ctrl=true shift=false` | `C-?` → undo-redo | **C-? 与 C-/ 可区分** |
| Ctrl+/ | `key="/" key_char=None ctrl=true` | `C-/` → undo | |
| Ctrl+Shift+- | `key="_" ctrl=true` | `C-_` → undo | |
| dead_acute → a（us intl） | 先预编辑 `´`，再 `key="aacute" key_char="á"` | SelfInsert（按 key_char） | **死键组合能输入 á** |
| AltGr+e（us intl） | `key="e" key_char="é"` | SelfInsert | AltGr 归入文本输入 |

由此修正自插入判定：不能看 `key` 是否单字符，要看 `key_char` 是否可打印；
死键中间态（plain、`key_char=None`、单字符 key）归为 `Composition`，消费但不回显 undefined。
`keys.rs` 的 `KeyStroke` 身份比较忽略 `key_char`（它是载荷，不参与 keymap 查找），并有单测。

## Layer 3：真实 fcitx5 / Hyprland（2026）

用真实 Hyprland 会话 + 用户 fcitx5（pinyin）跑 `run-real.sh`（`SPIKE_REAL=1` 才允许，焦点每次校验，
结束时恢复 IME 输入法与激活状态）。真实 text-input-v3 路径：

1. 预编辑 `n i h a o` 到达 `replace_and_mark_text_in_range`，`marked_text_range` 为 `0..6`，
   文本临时为 `ni hao`；**undo 记录仍为 0、dirty=false**。
2. 空格提交：`replace_text_in_range range=None marked=0..6 text="你好"`，`CHANGE text="你好"`；
   undo=1、dirty=true。提交按“替换预编辑、旧文本为空”记录，undo 只删除已提交文本。
3. 提交后 `C-/`：`REPLAY undo 0..6 <- ""`，文本回空，**不再 panic**（此前按错位范围记录会越界）。
4. 预编辑中 Escape：`replace_text_in_range Some(0..6) marked=0..6 text=""`，不建 undo 组、不 dirty。
5. 跨目标/焦点：`accepts_text_input=false` 会让 GPUI 调 `disable_ime`，平台随后取消组合（异步）。

### D17：组合未提交时拒绝命令（已批准）

公开 API 没有“主动提交平台 IME 组合并等待完成”的入口。实测“组合中保存”会把预编辑当正文
（`Saving file (spike): 6 chars`），随后取消组合使 buffer 变空且仍 clean → 真实文件会写坏。
因此按用户批准的策略：`marked_text_range()` 非空时，命令与前缀启动一律拒绝，
回显 `Input method composition is active; finish it first`，保持组合与 buffer 原样；
只有观察 / 组合探针例外。

- `cases/s4-ime-barrier.*`：用 f10/f11 假标志测策略分支（headless 无法稳定持有合成 marked text，
  GPUI 会在虚拟键盘 leave 时删掉它），断言 save / C-x / switch 被拒绝，关闭标志后 save 正常。
- `real-cases/s4-ime-save.*`：真实 fcitx5，`REFUSE save-buffer preedit=0..6`，组合保留；Escape 取消后
  同一命令保存 `0 chars`。
- 仍未覆盖：窗口关闭、点击其它窗口、系统切走焦点不经过命令入口；M1 必须在 close / focus 路径做同样检查。
  若上游未来提供 commit/cancel + 完成通知，可升级为“先收敛再执行”。

## 鼠标拖选（virtual pointer，layer 2 + 真实合成器）

新增 `scripts/vptr/`（wlr virtual pointer，绝对坐标 + 按键）。`cases/s4-mouse.*` 在 headless sway：
初始 `hello world`，`spike-sync-selection` 从 EditorState 的 `cursor()` + `selected_range()` 导入：

| 操作 | EditorState | 导入结果 |
|---|---|---|
| 正向拖选 | `cursor=11 sel=1..11` | point=11、mark=1、active=true |
| 反向拖选 | `cursor=1 sel=1..11` | point=1、mark=11、active=true（方向保留） |
| 单击起始处 | `cursor=0 sel=0..0` | point=0、mark 保留 11、active=false |

真实 PGTK Emacs（headless sway + 同一 vptr）为 1-based point=12/mark=2（正向）、point=2/mark=12（反向）、
单击后 point 移动、mark 保留、active=false，与上表 0-based 数值一致。
限制：这是虚拟指针，不是物理鼠标；尚未验证拖选与 IME / caret 同步的交错。

## 多 buffer 隔离（S3）

`cases/s3-buffers.*`：A 空、B `XY`，各自编辑 / 设 mark / undo。切换是命令边界；
`BUFFERS` 行同时打印两个 buffer 的 text / point / mark / active / undo。
结果：A 两次 undo 后 `aa → ""`，B 仍 `XYbb`；切到 B undo 只回 `XY`；A/B 的 mark 与 active 互不串用。
Emacs 基准 `buffers` case 同样保留 A 的 point/mark/active 并隔离 undo。

## 仍未覆盖 / 未解决

1. **mark 跟随编辑 / undo**：当前是 offset，未按 marker 调整；M1 必补。
2. **D17 的非命令入口**：窗口关闭、焦点被切走、WM 强退。
3. **物理键盘**：用的是上传真实 keymap 的虚拟键盘（协议层等价），不是字面硬件；
   如需最终确认，可由用户按几下 C-?/Shift/死键。
4. **系统保留键**（WM 层快捷键）分类未测。
5. **真实 case 的可移植性**：`real-cases/*.expected` 依赖本机 Hyprland + fcitx5 pinyin + us 布局，
   换机器/升级 fcitx5 后需人工审阅并 `UPDATE=1`。
6. **性能**：只做了功能验证，没有按键到显示的延迟测量。
