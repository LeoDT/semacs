# 架构设计

状态：方案 B 为候选实现路径。S1–S5 的机制验证见 `spikes/EVIDENCE.md`；D16 自建 undo 与 D17 IME 组合屏障已获批准。
mark 已按 marker 语义验证；D17 的窗口关闭入口已验证，真实焦点切换路径仍待按第 6 节要求复核。
若公开 API 无法满足约束，暂停正式实现，按 D14 重新决策，不默认增加保真度差异。

## 1. 分层

```
        ┌────────────────────────────────────────────┐
        │ AppShell（frame / window / 当前 view）      │
        │  - 命令执行出口                             │
        │  - mode line / echo area / minibuffer       │
        └───────────────┬────────────────────────────┘
                        │
        ┌───────────────▼────────────┐   ┌──────────────────────┐
        │ keys（纯逻辑键位引擎）      │   │ command（命令注册表） │
        │ KeySeq / Keymap / 解析      │──▶│ name / doc / execute  │
        └───────────────┬────────────┘   └──────────┬───────────┘
                        │ 拦截/消费                  │
        ┌───────────────▼────────────────────────────▼───────────┐
        │ BufferView（每个 Emacs window 一个）                    │
        │ region 渲染 / caret 同步 / 当前 buffer 绑定           │
        └───────────────┬────────────────────────────────────────┘
                        │
        ┌───────────────▼────────────────────────────┐
        │ Buffer（文本、文件、dirty、模式、undo 入口） │
        │  - 持有编辑状态，暴露 Emacs 级编辑 API       │
        └───────────────┬────────────────────────────┘
                        │
        ┌───────────────▼────────────────────────────┐
        │ gpui-kit EditorState（文本 + caret + 排版   │
        │ + IME + 滚动 + RangeDecoration）            │
        └────────────────────────────────────────────┘
```

职责边界：

- **keys / command / buffer 的 API 不依赖 GPUI 的具体 widget**，方便单测。
- **只有 `buffer/` 内部的适配层**可以接触 `Entity<EditorState>` / `Entity<InputState>`。命令层不得直接操作它们。
- **Buffer** 持有 v1 每个正文 buffer 的编辑状态（point / mark / 激活状态 / goal column）；**BufferView** 绑定当前 buffer 并负责渲染与同步，不另存一份语义状态。
- 正文与 minibuffer 通过同一 **EditTarget** 窄接口执行编辑命令；minibuffer 的输入控件也封装在适配层内。
  `EditorState` / `InputState` 的 cursor 承载 caret 与平台交互状态，来源与完成时机按第 6 节同步协议区分，不能假定它只是无副作用的绘制字段。

## 2. 预期目录结构

```
Cargo.toml
src/
  main.rs           bootstrap：application / init / open_window
  app.rs            AppShell：当前 view、命令执行出口、focus 路由、全局 kill ring / 命令链
  platform.rs       键名规范化、可打印字符判定、平台差异
  keys/
    mod.rs          KeyStroke / KeySeq / Keymap / 解析 / 匹配 / 单测
    emacs.rs        默认 global keymap
  command.rs        CommandSpec 注册表（M-x 与将来的 C-h 数据源）
  buffer/
    mod.rs          Buffer：编辑状态、文件、dirty、undo 委托
    view.rs         BufferView：当前 buffer 绑定、region 渲染 / caret 同步
    target.rs       EditTarget、EditingState、正文 / minibuffer 底座适配
    edit.rs         Edit 计算（kill / motion / 缩进等纯函数）
    kill_ring.rs    kill ring 数据结构（由 AppShell 持有）
  mode.rs           MajorMode trait 与模式注册
  frame.rs          Emacs window（v1 只有 Leaf）与 mode line
  minibuffer.rs     read-from-minibuffer（M-x / find-file）与补全
  files.rs          打开 / 保存 / 编码
  echo.rs           echo area 与 pending 键显示
docs/
.pi/tasks/          任务跟踪（实现阶段）
```

单 crate；若将来编译时间成为问题再拆 workspace。

## 3. 按键分发链路

GPUI 提供 `App::intercept_keystrokes`：它在 keymap / action 解析**之前**触发。
按参考源码，调用 `cx.stop_propagation()` 可同时阻止 GPUI action 和平台层把 `key_char` 插入 buffer。
这是 S1 要在 crates.io 实际依赖及 Wayland 上验证的假设，不等价于所有 IME 输入都经过按键事件。

```
cx.intercept_keystrokes(move |event, window, cx| {
    let consumed = shell.update(cx, |shell, cx| {
        shell.handle_keystroke(&event.keystroke, window, cx)
    });
    if consumed {
        cx.stop_propagation();
    }
});
```

允许文本输入的按键交给平台路径，由当前输入目标的 `EntityInputHandler` 处理。
IME 预编辑和提交可能直接调用输入 handler，不能只靠 interceptor 跟踪或拦截。
路由先判断当前焦点、平台输入状态与系统保留键，再执行下面的编辑器键位解析；
minibuffer 是独立输入目标，不把其输入或编辑命令发送到正文 buffer。

```
fn handle_keystroke(&mut self, ks, window, cx) -> bool {
    match self.keymaps.resolve(&self.pending, ks) {
        Resolution::Prefix => { self.pending.push(ks); self.echo.show_prefix(); true }
        Resolution::Execute(cmd) => { self.pending.clear(); self.execute(cmd, window, cx); true }
        Resolution::Undefined => { self.pending.clear(); self.echo.error_undefined(); true }
        Resolution::SelfInsert => false,   // 仅已确认的文本输入路径
    }
}
```

命令统一出口负责选择输入目标、执行命令和完成后的状态收敛，但**不无条件写回 caret**：

- 自有 motion / 编辑完成后，适配层按需要把 point 写给底座并更新 region / 滚动。
- 平台输入、鼠标操作和 undo / redo 后，先从底座取得已完成的变更与 cursor，再更新语义状态。
- `window.dispatch_action` 或其它异步 action 的调用返回不视为编辑已完成；undo / redo 已自建（D16），
  不再依赖 action。平台输入、鼠标和回放的完成观测与后续操作顺序见第 6 节。
- `set_selected_range` 会调用 `move_to` 并打断底座 undo 合并，不能同时承担所有同步和事务边界职责。
  caret 同步、滚动与 undo 分组分别按行为验收；公开 API 若无法兼顾，记录为阻断问题而非隐式降级。

## 4. 键位引擎

纯数据结构，不依赖 GPUI：

```
struct KeyStroke { key: String, ctrl: bool, alt: bool, shift: bool, super_: bool }
type KeySeq = Vec<KeyStroke>;

struct Keymap {
    name: &'static str,
    parent: Option<Arc<Keymap>>,           // global < major mode < minor / transient
    bindings: HashMap<KeySeq, Binding>,
}

enum Binding { Command(CommandId), Prefix }
enum Resolution { Execute(CommandId), Prefix, Undefined, SelfInsert }
```

规则：

- 同一 keymap 的一个节点要么是命令，要么是前缀；注册时拒绝命令与子序列冲突，不用等待规则掩盖冲突。
  允许显式声明尚无子绑定的前缀（如 v1 的 `C-c` / `C-h`），不能仅靠现有命令的子序列推导所有前缀。
- 多层 keymap 的覆盖与前缀查找须有测试，不把父表前缀无条件置于子表命令之前。
- 空前缀下未绑定、可打印，且无 control / Meta / super → `SelfInsert`；Shift 产生的字符可自插入。
- 普通 Meta 组合是命令键，不因存在 `key_char` 就自插入；AltGr 只有经平台识别为文本输入时才例外。
- 非空前缀下未绑定，或当前没有合法文本输入路径 → `Undefined`，消费按键并回显，不静默吞掉。
- 死键、IME 预编辑与系统保留键在平台 / 焦点路由层分类；不以“未知修饰键”概括这些事件。
- `C-g` 清空 pending、取消当前交互并停用 region，但不删除已存在的 mark 位置。

键名解析器 `parse_key_sequence("C-x C-s")` 自己实现并单测，支持 `C- M- S- s-`。

不使用 GPUI 的 keymap / action 作为主分发器，原因见 [决策 D4](decisions.md)。

## 5. 命令注册表

```
struct CommandSpec {
    name: &'static str,   // "forward-char"，M-x 使用
    doc: &'static str,
    execute: fn(&mut AppShell, &mut Window, &mut Context<AppShell>),
}
```

- 静态注册表 + 按名字查找；`M-x`、将来的 `C-h k` / `C-h f` 共用这一份数据。
- 命令操作 `AppShell`（对应 Emacs 的 selected window / current buffer），不直接持有某个 view。
- motion / 编辑 / kill / yank / undo 命令只调用当前 `EditTarget` 的窄接口，正文与 minibuffer 共用命令实现。
- 文件与 buffer 管理命令调用 `Buffer` / AppShell 的专用接口，不把文件能力强加给 minibuffer。
- AppShell 持有全局 kill ring 与连续命令链状态；正文各 buffer 和 minibuffer 共享 kill ring，undo 历史则各自独立。

## 6. Buffer、BufferView 与公共编辑目标

`EditTarget` 是语义接口而非 widget 抽象，最小能力为：稳定目标标识、读取文本与编辑状态、
设置 point / mark、应用 `Edit`、请求 undo / redo、取得编辑完成结果及组合输入状态。
底座事件归一化为携带目标标识、来源、替换范围 / 内容和完成状态的结果，再交给公共语义层处理。
接口不暴露 `EditorState` / `InputState` 或 GPUI action；具体 Rust 签名由 S3 / S4 验证后确定。

- 正文目标由 `Buffer` 实现，内部适配 `EditorState`；minibuffer 目标由 `MinibufferTarget` 实现，内部适配 `InputState`。
- 两者共用 `EditingState` 和纯文本编辑算法。adapter 只处理控件差异，不复制 kill / motion / mark 的语义实现。
- prompt / 补全属于 minibuffer 会话；prompt 不属于可编辑文本。文件身份、保存点属于正文 Buffer。
- minibuffer 的 `RET` 确认与 `TAB` 补全由其 keymap / 会话处理；不借此放行底座默认 CUA 编辑命令。

下面是状态归属示意，不是已验证可编译的底座接口：

```
impl Buffer {
    fn text(&self) -> &Rope;
    fn len(&self) -> usize;
    fn slice(&self, range: Range<usize>) -> String;
    fn apply(&mut self, edit: Edit, window, cx);   // 自有编辑；分组规则见 D16
    fn undo(&mut self, window, cx);                // 自建 change 历史，不派发底座 action
    fn redo(&mut self, window, cx);
    fn is_modified(&self) -> bool;
    fn save(&mut self, window, cx) -> io::Result<()>;
}

struct EditingState {
    point: usize,
    mark: Option<Marker>,      // 不是 offset：insertion type nil，随文本变更移动
    mark_active: bool,
    goal_column: Option<usize>,
}

struct Buffer {
    editing: EditingState,
    // 其余文本适配、文件、dirty 与历史字段省略。
}

struct BufferView {
    buffer: Entity<Buffer>,
    region: RangeDecorationCollection,
    _subs: Vec<Subscription>,
}
```

- point / mark 是**我们自己的模型**，不依赖底座的 selection 表示。
  mark 使用 marker 语义：在其前面插入会后移，在其位置插入不动，覆盖它的删除会坍缩到删除起点；
  删除时记录塌缩偏移，undo 只有在 mark 仍位于该起点时才恢复。更新点必须放在适配层的唯一文本变更入口，
  因为平台输入与历史回放不经过命令层，不能在每条命令后各自修一次。
- v1 每个正文 Buffer 保存自己的 `EditingState`；切换 A → B → A 不重置 A 的 point / mark，也不把 A 的状态复制给 B。
  mark 与激活状态是 buffer-local；切换命令引起的激活状态变化、goal column 重置按固定 Emacs 基准处理，不等于无条件恢复旧激活状态。
- 切换前先完成旧目标的输入 / 编辑，解除 view 对旧 Buffer 的高亮与渲染订阅，保留 Buffer 自身的变更观测；
  再绑定新 Buffer、从其状态同步 caret 并重建高亮。
  每个 Buffer 的底座实例及滚动位置保留；只有 kill-buffer 才释放其编辑状态和历史，迟到事件不得写入新目标。
  未来 split 的 window-point 另行设计；当前 buffer-local point 只覆盖单正文 view，不宣称解决了多视图问题。
- region 高亮用 `RangeDecoration`（Fill），其 range 会随编辑自动调整；不把 EditorState
  的 selection 当作 region，因为公开 API 无法表达"point 在 mark 之前"的反向 region
  （`set_selected_range` 强制 caret 落在末端）。
- mark 位置与激活状态独立：`None` 表示从未设 mark，`mark_active` 控制 region 是否激活。
  `C-g`、undo / redo、平台输入与 IME 提交的停用只清 active，不等于把 mark 设为 `None`；
  mark 仍随文本变更移动。
- 预编辑文本视为临时层：开始 / 更新 / 取消不移动 mark，提交按一次插入更新。
- 输入与同步协议的约束：
  - 文本以 `EditorState` 为准；自有命令的 point 意图与底座产生的 cursor 更新必须区分来源，不能互相覆盖。
  - 自有编辑提供 `Edit { range, replacement }`；所有位置以 UTF-8 字节偏移表示，并保持字符边界。
  - 平台输入和历史回放也必须取得足够的替换范围 / 替换内容信息，才能更新 mark。
    IME 可指定替换范围；禁止用“旧 point + 新 cursor + 字节长度差”推断一般编辑。
  - 预编辑、提交、取消分别处理；取消后文本、mark 和 dirty 状态应恢复到相应状态。
    底座文本可能包含临时预编辑，不得因它是显示权威就直接把它当作可保存的已提交文本。
  - 鼠标拖选结束后，适配层按 cursor 与 selection 的方向导入 point / mark / 激活状态，再转换为 region 高亮。
    不在每次 `cx.observe` 通知中无条件清 selection，以免破坏拖选锚点或 IME selection。
  - 自有同步不应反向触发第二次语义更新；通用 `Change` / observe 通知本身不足以证明来源、范围或操作完成。
  - 平台输入提交也参与语义操作边界，须打断连续 kill 等命令链；不能因绕过命令注册表而遗漏。
- S4 必须验证外部变更的观测机制与完成时机，产出覆盖鼠标、IME、自有命令和历史回放的同步方案；
  当前不预设公开 API 已满足这些要求。
- `RangeDecoration` 仅用于非空高亮；参考源码会过滤空范围，不能用空 decoration 充当通用 marker。
- 命令层禁止直接接触 `Entity<EditorState>` / `Entity<InputState>`，为将来的 model/view 拆分留缝。

### 组合输入与命令的串行化

- 平台 / IME 自己消费的候选操作不作为编辑器命令执行。当前公开 API 没有“主动提交组合并等待完成”的入口，
  因此按 [D17](decisions.md#d17-ime-组合屏障命令拒绝关闭--失焦取消)：`marked_text_range()` 非空时，
  命令、前缀启动、保存、切换 buffer、进入 / 退出 minibuffer、kill / quit 一律拒绝并回显
  `Input method composition is active; finish it first`，组合与 buffer 保持原样。
- 关闭 / 失焦不是命令入口：未提交的预编辑不是输入，按取消处理。
  `Window::on_window_should_close` 先取消组合再允许关闭；内部 `Window::remove_window()` 同样接入。
  焦点被切走时以平台在 text-input leave 的取消为准，保留主动取消兜底；不回写 buffer、不变 dirty。
- 所有回调绑定原目标 / 会话代次，迟到提交不得按当前焦点重新解释，也不能借焦点变化推定输入已完成。
  若将来上游提供公开的 commit / cancel + 完成通知，可升级为“先收敛再执行”。
- 事件必须关联原目标；目标销毁前拒绝或取消组合；已结束会话的迟到回调不得修改另一个 buffer 或新的 minibuffer 会话。
- 保存只读取已提交文本，不能把临时预编辑落盘；取消后验证内存、磁盘与保存点没有误变更。
- pending 前缀期间直接到达 handler 的文本不能无条件自插入。前缀启动前的组合输入先按 D17 处理；
  前缀期间新到达的文本走受前缀约束的输入解析，按基准完成绑定或报 undefined，不绕过键位引擎修改文本。

## 7. undo

- undo / redo 由我们自己的 change 记录实现（[D16](decisions.md#d16-自建-undo)），
  不派发 `EditorState` 的 `Undo` / `Redo` action；每个编辑目标持有独立历史，dirty 与保存点由 change 记录派生。
- 每个 `Change` 记录 `{range, old, new, kind}`；命令意图区分 Typing / DeleteBackward / DeleteForward / Other。
  连续同类合并，其它命令插入边界，undo / redo 不与输入合并，undo 后输入开新组并清空 redo。
- replay 通过同一个 `EditTarget` 应用，带 `replaying` 标志避免自我记录；mark 也在这条路径上更新。
- 绑定与 Emacs 31 一致：`C-/` / `C-_` / `C-x u` = undo；`C-?` / `C-M-_` = redo。
- 已知差异：region 激活时仍执行全局 undo（undo-in-region 属 F3）。
- undo 恢复组开始前的 point；redo 回到组内最早变更的起点（与 Emacs 对齐，`Group::redo_point()`）。
- 无可撤销内容时回显 `No further undo information`，不静默失败。

## 8. minibuffer 与 echo area

- 底部一条：mode line + echo area；minibuffer 激活时占用 echo area 的位置。
- `MinibufferState { prompt, kind, target: MinibufferTarget, candidates }`；`MinibufferTarget` 在 `buffer/` 适配层封装 `InputState`、独立 `EditingState` 与历史。
- v1 读取函数：`M-x`（命令名补全）、`C-x C-f` / `C-x C-w`（路径读取）、`C-x b` / `C-x k`（buffer 名补全）。
- `RET` 确认、`TAB` 补全、`C-g` 取消；更完整的补全交互在 M3 打磨。
- 键位分发在 minibuffer 激活时切换到 minibuffer 自己的 keymap，但 `C-g` / 前缀显示沿用全局行为。
- minibuffer 激活时是当前输入目标；正文 point / region 保留，不能把 minibuffer 的输入、删除或 undo 路由到正文。
  确认后执行待完成命令，取消则不执行；两种路径都恢复原焦点并清理 pending。
- S2–S4 先用最小会话验证公共 `EditTarget`：`C-a` / `C-e`、删除、`C-k` / `C-y`、mark 与 undo；
  正文 → minibuffer → 正文的 kill / yank 共用 kill ring，undo 不跨目标，prompt 不可被编辑。
- `M-x` 与上述读取 / 补全流程在 M2 明确实现和验收；补全替换也通过目标编辑入口，不直接改 widget。
  会话确认 / 取消必须经过第 6 节的输入完成屏障，之后才能执行待完成命令或销毁目标。

## 9. 平台适配

全部集中在 `src/platform.rs`：

- 键名规范化（例如 `C-?`、`RET` / `C-m`、`TAB` / `C-i`）；
- 可打印字符 / 自插入判定（AltGr、死键、IME 预编辑）；
- 平台默认字体、mono 字体、主题；
- 平台专属系统键（v1 只保留窗口管理）。
- 开发与验收在 Wayland；X11 保持可运行。macOS / Windows 适配时只改这一层与少量 modifier 命名。

## 10. 为什么选 B（trade-off 汇总）

一句话：**用"复用最难写的输入与排版层"换"编辑语义和视图实例被绑在 widget 里"**。

| 能力 | 来源 | 说明 |
|---|---|---|
| 文本存储与编辑 | EditorState | Rope、UTF-8 / UTF-16 转换、编辑应用 |
| undo 事务 | 我们（D16） | 自建 change 记录；EditorState 只负责应用文本 |
| 排版与绘制 | EditorState | 软换行、行号、caret、选区、滚动、虚拟化、鼠标命中测试 |
| 输入 | EditorState + 平台 | IME / 预编辑、剪贴板、无障碍 |
| 装饰 | EditorState | RangeDecoration / TextDecoration / gutter，随编辑自动跟踪 |
| 键位 / 前缀 / 命令 | 我们 | keys + command |
| point / mark / region | 我们 | EditingState（正文归 Buffer、minibuffer 归会话目标）+ view 高亮 |
| motion / kill / yank / minibuffer | 我们 | buffer/edit.rs、kill_ring、minibuffer |
| buffer / mode / window / 文件 | 我们 | frame、mode、files |

被否方案：

- **A（直接用 EditorState 的 selection 与 Move/Select action）**：最快，但 motion、word/line 边界、
  goal column 的语义都是 Zed 的，保真度上限低，后续修改要动所有命令。
- **C（自研文本引擎 + 自研 view）**：控制力最强，但要自己实现 IME、命中测试、滚动 / 虚拟化、
  无障碍、caret 绘制，数周工作量且长期消耗边角案例；v1 目标不允许。

已知代价与待验证限制：

1. point / mark 需要自建与同步（D7、D18）；
2. undo 由我们维护（D16），不再是底座限制；redo point 已对齐，undo-in-region 属 F3；
3. 同一 buffer 多窗口不可行（D9）；
4. 每个 buffer 一个 widget 实例，命令层不能直接摸 `EditorState`，用 `Buffer` 窄接口隔离。

## 11. EditorState 公开 API 的事实与缺口

参考源码中可见的 API（不表示已在 crates.io 依赖上构建或运行验证）：

- `text()` / `cursor()` / `selected_range()` / `set_selected_range()` / `insert()` / `replace()` / `set_value()` / `focus()`
- `scroll_offset()` / `set_scroll_offset()` / `visible_row_range()` / `line_height()` / `range_to_bounds()`
- `create_range_decorations_collection()` / `create_decorations_collection()` / gutter 版本
- `set_auto_close()` / `set_line_number()` / `soft_wrap()` / `readonly()`
- `InputEvent::Change`
- `Undo` / `Redo` action

缺口与验证路径：

| 缺口 | 处理 / 验证路径 |
|---|---|
| selection 无法表达反向 region（`set_selected_range` 强制 `reversed = false`） | point / mark 自建，region 用 `RangeDecoration`（D7） |
| `undo` / `redo` / 事务控制与编辑意图非公开 API | 已由 D16 绕过：自有 change 记录，编辑经 `EntityInputHandler` 应用；不依赖 action 与 EditIntent |
| point / scroll / focus 绑在 state 上 | 同 buffer 多窗口推迟（D9） |
| 没有通用 marker / 外部变更流 | mark 在我们这里维护（D18）；所有文本变更经 `EditTarget` 同一入口。空 `RangeDecoration` 会被过滤，不能兜底 |
| 窗口关闭没有专用 hook | `Window::on_window_should_close` 可在关闭前执行取消逻辑并允许关闭（D17）；内部 `remove_window` 仍需自己接入 |
| 焦点切走没有 veto | 只能观察 activation；回调必须绑定原目标 / 会话代次，不能按当前焦点重解释 |
| 没有 narrowing / indirect buffer / text property | 明确非目标 |

## 12. 未来替换文本引擎的成本

如果将来必须支持"同一 buffer 多窗口独立 point"，三种路径：

1. 每个 view 一个 `EditorState` + 自建文本模型同步：O(n) 全量替换，不可行；
2. 自己实现 view（Rope + undo 自研，排版 / IME / 命中测试自绘，或基于 `text::editor_host`）：
   1–2k 行以上，是**真正的方案**；
3. 改上游 `gpui-base` 把 model / view 拆开：2 万行级重构，不建议。

当前选择：v1 只有一个正文 view，可切换多个 buffer；所有 split 均在 v1 之后的 M4 评估。
若开展不同 buffer 的 split，仍需定义 `C-x 2/3` 默认复制当前 buffer 与禁止同 buffer 多视图之间的冲突，
不得在“纯 Emacs 键位”名义下静默替换语义。窄接口用于收敛未来替换成本，不保证不需要上层调整。

## 13. spike 验证状态与剩余门禁

以 [实施计划的 S1–S5](plan.md#1-先做-spike不写正式代码) 为唯一验收清单；逐项证据在 `spikes/EVIDENCE.md`。
已通过：interceptor 边界、前缀 / undefined / C-g、自建 undo 分组 / 保存点 / redo point、平台输入与 IME 观测、
mark marker 语义、鼠标拖选导入、多 buffer 隔离、真实键位分类、IME 命令屏障、关闭 / 失焦取消、
真实 fcitx5 提交后的 mark。
可选剩余：真实硬件键盘与 WM 保留键确认；WM 强退 / 进程崩溃未测。
S3 / S4 无法满足 F1 时，按 D14 暂停并确认，不自动接受新差异或启动自研 view。

## 14. 测试策略

- `keys`：键名解析、前缀匹配、undefined / self-insert 判定的纯单测。
- `buffer/edit.rs`：kill / motion / 缩进等文本变换的纯单测（直接对 Rope 断言）。
- 交互测试：使用 gpui-kit 的 `test-support` 与 `VisualTestContext`，
  `cx.simulate_keystrokes("ctrl-f")`、`cx.simulate_keystrokes("ctrl-x ctrl-s")`，
  断言 buffer 文本、point、mark、echo 状态。
- undo 分组与 mark 跟随使用固定基准脚本，记录 Emacs 环境及 semacs 真实链路结果；
  [保真度文档](emacs-fidelity.md) 的矩阵是验收要求，具体结果与证据位置以 `spikes/EVIDENCE.md` 为准。
