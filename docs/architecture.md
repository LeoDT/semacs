# 架构设计

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
        │ point / mark / goal column / region / caret 同步       │
        └───────────────┬────────────────────────────────────────┘
                        │
        ┌───────────────▼────────────────────────────┐
        │ Buffer（文本、文件、dirty、模式、undo 入口） │
        │  - 只暴露 Emacs 级编辑 API                  │
        └───────────────┬────────────────────────────┘
                        │
        ┌───────────────▼────────────────────────────┐
        │ gpui-kit EditorState（文本 + caret + 排版   │
        │ + IME + 滚动 + RangeDecoration）            │
        └────────────────────────────────────────────┘
```

职责边界：

- **keys / command / buffer 的 API 不依赖 GPUI 的具体 widget**，方便单测。
- **只有 Buffer 内部**可以接触 `Entity<EditorState>`。命令层不得直接操作它。
- **BufferView** 持有 Emacs 的 point / mark / goal column 与渲染入口；`EditorState` 的 cursor 只是 caret 的绘制载体。

## 2. 预期目录结构

```
Cargo.toml
src/
  main.rs           bootstrap：application / init / open_window
  app.rs            AppShell：当前 view、命令执行出口、focus 路由
  platform.rs       键名规范化、可打印字符判定、平台差异
  keys/
    mod.rs          KeyStroke / KeySeq / Keymap / 解析 / 匹配 / 单测
    emacs.rs        默认 global keymap
  command.rs        CommandSpec 注册表（M-x 与将来的 C-h 数据源）
  buffer/
    mod.rs          Buffer：文本编辑 API、文件、dirty、undo 委托
    view.rs         BufferView：point / mark / region / 渲染 / caret 同步
    edit.rs         Edit 计算（kill / motion / 缩进等纯函数）
    kill_ring.rs    kill ring
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
在其中调用 `cx.stop_propagation()` 会同时阻止 GPUI action 和平台层把 `key_char` 插入 buffer
（Linux X11 / Wayland 后端只在 dispatch 返回 `propagate == true` 后调用 `replace_text_in_range`）。

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

返回 `false` 的按键会落到平台文本输入路径，从而由 `EditorState` 的
`EntityInputHandler` 插入字符（IME、中文、死键都走这条路）。

```
fn handle_keystroke(&mut self, ks, window, cx) -> bool {
    match self.keymaps.resolve(&self.pending, ks) {
        Resolution::Prefix => { self.pending.push(ks); self.echo.show_prefix(); true }
        Resolution::Execute(cmd) => { self.pending.clear(); self.execute(cmd, window, cx); true }
        Resolution::Undefined => { self.pending.clear(); self.echo.error_undefined(); true }
        Resolution::SelfInsert => false,   // 交给平台 / IME
        Resolution::Ignored => true,       // 带修饰键的未知键：吞掉，绝不插入
    }
}
```

命令统一出口（注意"每条命令结束同步 caret"）：

```
fn execute(&mut self, cmd: CommandId, window, cx) {
    (self.commands[cmd].execute)(self, window, cx);
    self.active_view.update(cx, |view, cx| view.sync_caret_after_command(window, cx));
    self.echo.maybe_finish();
}
```

`sync_caret_after_command` 用 `state.set_selected_range(point..point, cx)` 完成三件事：

1. 把我们的 point 写给 EditorState 画 caret；
2. 触发 `move_to()`，打断 undo 合并，复现 Emacs 的"命令边界"；
3. 滚动到 point 可见。

## 4. 键位引擎

纯数据结构，不依赖 GPUI：

```
struct KeyStroke { key: String, ctrl: bool, alt: bool, shift: bool, super_: bool }
type KeySeq = Vec<KeyStroke>;

struct Keymap {
    name: &'static str,
    parent: Option<Arc<Keymap>>,           // global < major mode < minor / transient
    bindings: HashMap<KeySeq, CommandId>,
}

enum Resolution { Execute(CommandId), Prefix, Undefined, SelfInsert, Ignored }
```

规则：

- 精确匹配且同时是前缀时保持 pending（Emacs 行为）；
- 空前缀下未绑定、可打印、无 control / super → `SelfInsert`；
- 非空前缀下未绑定 → `Undefined`，消费按键并回显；
- 带修饰键的未知键 → `Ignored`，消费但不插入，避免 GPUI 的内置 action 意外触发；
- `C-g` 清空 pending 并取消当前交互。

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
- 命令实现只调用 `Buffer` / `BufferView` 的窄接口。

## 6. Buffer 与 BufferView

```
impl Buffer {
    fn text(&self) -> &Rope;
    fn len(&self) -> usize;
    fn slice(&self, range: Range<usize>) -> String;
    fn apply(&mut self, edit: Edit, window, cx);   // 一次 replace = 一个 undo step
    fn undo(&mut self, window, cx);                // 当前实现：派发 Undo action
    fn redo(&mut self, window, cx);
    fn is_modified(&self) -> bool;
    fn save(&mut self, window, cx) -> io::Result<()>;
}

struct BufferView {
    buffer: Entity<Buffer>,
    point: usize,
    mark: Option<usize>,
    goal_column: Option<usize>,
    region: RangeDecorationCollection,
    _subs: Vec<Subscription>,
}
```

- point / mark 是**我们自己的模型**，不依赖 `EditorState` 的 selection 表示。
- region 高亮用 `RangeDecoration`（Fill），其 range 会随编辑自动调整；不把 EditorState
  的 selection 当作 region，因为公开 API 无法表达"point 在 mark 之前"的反向 region
  （`set_selected_range` 强制 caret 落在末端）。
- 鼠标点击 / 拖选由 `EditorState` 内部处理；我们通过 `cx.observe` 读
  `cursor()` 与 `selected_range()` 反向同步成 point / mark，然后清掉原生 selection，
  并加 `syncing` 标志防回环。
- mark 的跟随（marker 语义）：
  - 我们自己的编辑直接按 `Edit { range, replacement }` 调整；
  - 平台自插入 / IME 提交发生在 point，可用"旧 point + 新 cursor + 字节长度差"定位并调整；
  - undo / redo 后清 mark（Emacs 也 deactivate mark）；
  - 将来如果规则变复杂，可把 mark 存成一个空 `RangeDecoration` 并用 `get_ranges()` 读回，
    让 `EditorState` 自动跟踪。
- 命令层禁止直接接触 `Entity<EditorState>`，为将来的 model/view 拆分留缝。

## 7. undo

- v1 委托 `EditorState`：`Undo` / `Redo` action，经 `window.dispatch_action` 派发。
- 绑定与 Emacs 31 一致：`C-/` / `C-_` / `C-x u` = undo；`C-?` / `C-M-_` = redo。
- 已知差异（连续 yank 的分组、undo-in-region）登记在 [保真度文档](emacs-fidelity.md)。
- 不自建 undo：平台自插入 / IME 提交不经过我们的编辑入口，精确记录 change 的成本过高。
  若将来自研 view，再实现 Emacs 语义的 undo。

## 8. minibuffer 与 echo area

- 底部一条：mode line + echo area；minibuffer 激活时占用 echo area 的位置。
- `MinibufferState { prompt, kind, input: Entity<InputState>, candidates }`。
- v1 读取函数：`M-x`（命令名补全）、`C-x C-f` / `C-x C-w`（路径读取）、`C-x b` / `C-x k`（buffer 名补全）。
- `RET` 确认、`TAB` 补全、`C-g` 取消；更完整的补全交互在 M3 打磨。
- 键位分发在 minibuffer 激活时切换到 minibuffer 自己的 keymap，但 `C-g` / 前缀显示沿用全局行为。

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
| undo 事务 | EditorState | 分组接近 Emacs，差异见 D8 |
| 排版与绘制 | EditorState | 软换行、行号、caret、选区、滚动、虚拟化、鼠标命中测试 |
| 输入 | EditorState + 平台 | IME / 预编辑、剪贴板、无障碍 |
| 装饰 | EditorState | RangeDecoration / TextDecoration / gutter，随编辑自动跟踪 |
| 键位 / 前缀 / 命令 | 我们 | keys + command |
| point / mark / region | 我们 | BufferView + RangeDecoration |
| motion / kill / yank / minibuffer | 我们 | buffer/edit.rs、kill_ring、minibuffer |
| buffer / mode / window / 文件 | 我们 | frame、mode、files |

被否方案：

- **A（直接用 EditorState 的 selection 与 Move/Select action）**：最快，但 motion、word/line 边界、
  goal column 的语义都是 Zed 的，保真度上限低，后续修改要动所有命令。
- **C（自研文本引擎 + 自研 view）**：控制力最强，但要自己实现 IME、命中测试、滚动 / 虚拟化、
  无障碍、caret 绘制，数周工作量且长期消耗边角案例；v1 目标不允许。

已知代价（均已登记）：

1. point / mark 需要自建与同步（D7）；
2. undo 分组委托给 EditorState，接受已登记差异（D8）；
3. 同一 buffer 多窗口不可行（D9）；
4. 每个 buffer 一个 widget 实例，命令层不能直接摸 `EditorState`，用 `Buffer` 窄接口隔离。

## 11. EditorState 公开 API 的事实与缺口

已确认可用：

- `text()` / `cursor()` / `selected_range()` / `set_selected_range()` / `insert()` / `replace()` / `set_value()` / `focus()`
- `scroll_offset()` / `set_scroll_offset()` / `visible_row_range()` / `line_height()` / `range_to_bounds()`
- `create_range_decorations_collection()` / `create_decorations_collection()` / gutter 版本
- `set_auto_close()` / `set_line_number()` / `soft_wrap()` / `readonly()`
- `InputEvent::Change`
- `Undo` / `Redo` action

缺口与绕过：

| 缺口 | 绕过 |
|---|---|
| selection 无法表达反向 region（`set_selected_range` 强制 `reversed = false`） | point / mark 自建，region 用 `RangeDecoration`（D7） |
| `undo` / `redo` / `begin_transaction` 是 `pub(super)` | undo / redo 用 action 派发；多步编辑实现成一次 `replace`，避免需要事务 |
| point / scroll / focus 绑在 state 上 | 同 buffer 多窗口推迟（D9） |
| 没有通用 marker / overlay | mark 用 offset + 已知编辑位置调整，必要时用空 `RangeDecoration` 跟踪；高亮用 decoration |
| 没有 narrowing / indirect buffer / text property | 明确非目标 |

## 12. 未来替换文本引擎的成本

如果将来必须支持"同一 buffer 多窗口独立 point"，三种路径：

1. 每个 view 一个 `EditorState` + 自建文本模型同步：O(n) 全量替换，不可行；
2. 自己实现 view（Rope + undo 自研，排版 / IME / 命中测试自绘，或基于 `text::editor_host`）：
   1–2k 行以上，是**真正的方案**；
3. 改上游 `gpui-base` 把 model / view 拆开：2 万行级重构，不建议。

当前选择：先只支持"每个 window 一个 buffer"，不同 buffer 的 split 不受影响；同 buffer
多窗口明确限制。`Buffer` / `BufferView` 的窄接口保证将来切换只改 `buffer/` 内部。

## 13. 待 spike 验证

1. `intercept_keystrokes` + `stop_propagation` 确实阻止平台插入；未拦截字符正常插入；IME 正常。
2. 前缀 `C-x` 的 pending 显示、`C-g` 取消、未定义键不插入。
3. 在 interceptor 中 `window.dispatch_action(Undo)` 的稳定性与连按顺序。
4. point / mark 同步与 `RangeDecoration`：反向 region、鼠标拖选后的转换、mark 跟随。
5. Wayland 下的 `C-?`、AltGr、死键键名与文本输入行为。

## 14. 测试策略

- `keys`：键名解析、前缀匹配、undefined / self-insert 判定的纯单测。
- `buffer/edit.rs`：kill / motion / 缩进等文本变换的纯单测（直接对 Rope 断言）。
- 交互测试：使用 gpui-kit 的 `test-support` 与 `VisualTestContext`，
  `cx.simulate_keystrokes("ctrl-f")`、`cx.simulate_keystrokes("ctrl-x ctrl-s")`，
  断言 buffer 文本、point、mark、echo 状态。
- undo 分组用一组固定脚本做回归（对照 [保真度文档](emacs-fidelity.md) 的表）。
