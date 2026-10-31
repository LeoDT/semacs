# 实施计划

## 1. 先做 spike（不写正式代码）

在正式铺 M0 之前，用一个最小示例验证下面 5 件事。任何一项失败都会改变方案，必须先验证。

| ID | 验证内容 | 通过标准 |
|---|---|---|
| S1 | `intercept_keystrokes` + `stop_propagation()` | 被消费的键不插入字符；未消费的可打印键正常插入；中文 IME 正常 |
| S2 | 前缀与取消 | `C-x` 进入 pending 并显示；`C-g` 取消；`C-x a` 报 undefined 且不插入 |
| S3 | undo 派发 | 在 interceptor 中 `window.dispatch_action(Undo)` 可重复、按序执行，并同步 caret |
| S4 | point / mark / region | 反向 region 正确；鼠标拖选后能转换成我们的 point / mark；mark 能跟随编辑 |
| S5 | Wayland 键名 | `C-?`、AltGr、死键、`RET` / `TAB` 的 keystroke 形态符合预期 |

## 2. 里程碑

里程碑按"可验收的行为"划分，不按时间估算。每个里程碑结束都要能运行、能测试。

### M0 骨架

内容：

- cargo 工程、依赖、应用启动、窗口与主题；
- 命令行文件参数或 scratch buffer；
- `EditorState` 渲染、mode line、echo area；
- 关闭窗口的默认行为。

验收：`cargo run -- <file>` 能显示文件内容，可以输入文字，中文输入正常。

### M1 键位引擎 + 基础编辑

内容：

- `keys`：`parse_key_sequence`、`Keymap`、前缀匹配、undefined / self-insert / ignored 判定；
- interceptor 接入、命令注册表、统一命令出口与 caret 同步；
- 基础命令：字符 / 行 / 词 / buffer 首尾移动，删除，`C-k`，`C-w` / `M-w` / `C-y`，
  `C-SPC`，`C-g`，undo / redo；
- echo area 显示 pending 前缀与错误。

验收：

- 纯键盘完成基本编辑；
- 键位引擎单测通过；
- 交互测试覆盖 `ctrl-f`、`ctrl-x ctrl-s` 之类的序列；
- 未定义键不插入字符。

### M2 文件与 buffer 管理

内容：

- `find-file` / `save-buffer` / `write-file` / `switch-to-buffer` / `kill-buffer`；
- dirty 标记、`C-x C-c` 的确认；
- minibuffer 的读取与基础补全；
- buffer 列表与切换。

验收：不碰鼠标完成"打开 → 编辑 → 保存 → 切换 → 退出"。

### M3 Emacs 语义补齐

内容：

- kill ring 的追加规则、`M-y`（如确定做）；
- `C-u` 数字参数、按键重复；
- isearch（`C-s` / `C-r`，按讨论结果）；
- mode line 信息与回显细节；
- 已知差异的回归测试。

验收：对照 [保真度清单](emacs-fidelity.md) 的 F1 项逐条通过；F2 / F3 项有明确结论。

### M4 多窗口与打磨

内容：

- `C-x 1/2/3/o`，不同 buffer 的 split；
- 同 buffer 多窗口的限制提示；
- 根据实际使用感受决定是否启动"自研 view"（方案 C）；
- 性能检查：大文件打开、滚动、按键延迟。

验收：split 下焦点 / 命令路由正确；同 buffer 限制行为明确。

### M5 diff 与 magit（后续阶段）

内容：

- diff 查看器（gpui-kit `component::diff`）；
- magit 风格只读 buffer：section 折叠、`TAB`、`RET`、transient keymap、刷新；
- 只读 buffer 的 keymap 机制在 M1 的键位引擎中已具备，不另起框架。

## 3. 测试策略

- **纯单测**：键名解析、序列匹配、undefined 判定、文本变换、kill ring、文件名处理。
- **交互测试**：gpui-kit 的 `test-support` + `VisualTestContext`，用
  `cx.simulate_keystrokes("ctrl-x ctrl-s")` 驱动真实分发链，断言文本 / point / mark / echo。
- **对照测试**：undo 分组等行为用固定脚本对照 [保真度文档](emacs-fidelity.md) 的表。
- **手工验收**：Wayland 下的 IME、系统剪贴板、字体渲染。

## 4. 任务跟踪

实现阶段在 `.pi/tasks/` 下建立跟踪文件，按里程碑维护：

```
- [ ] S1 intercept_keystrokes 验证
- [ ] S2 前缀与取消
...
```

## 5. Definition of Done（每个里程碑）

- `cargo test` 全绿；
- 新增行为有对应测试；
- 文档中的"待决 / 待验证"项有更新；
- 在 Wayland 上手工验证过关键交互；
- 没有把 EditorState 泄漏到命令层。
