# Emacs 保真度

本文定义"像 Emacs 到什么程度"，是本项目**最重要的约束**。实现中出现"要不要和 Emacs 一样"的争议时，先在这里判定级别。

参照实现：GNU Emacs 31.1（本机 `emacs -Q` 实测）。平台差异（字体、滚动、IME）单独讨论。

## 1. 判定方法

一个行为是否"像 Emacs"，按下面的顺序判断：

1. **键序列相同**：同样的按键（含前缀）触发同名命令。
2. **可观察结果相同**：执行后 buffer 文本、point、mark、kill ring、echo area、mode line 的状态相同。
3. **交互流程相同**：前缀等待、`C-g` 取消、未定义键报错、minibuffer 的进入 / 退出 / 补全流程相同。
4. **不可观察的差异不算差异**：内部实现、数据结构、绘制方式可以不同。

## 2. 保真度分级

### F1：必须一致（不一致即 bug）

- 键序列与命令名：`C-f` = `forward-char`，`C-k` = `kill-line` 等。
- 前缀键：`C-x`、`C-c`、`M-`、`C-h` 作为前缀的等待与完成行为。
- `C-g` = `keyboard-quit`：取消前缀、取消 minibuffer、取消 mark、echo "Quit"。
- 未定义键：消费按键、不插入字符、echo 形如 `C-x a is undefined`。
- 自插入：无修饰键的可打印字符插入自身；带修饰键或非打印键不插入。
- point / mark / region 的基本语义：`C-SPC` 设 mark；移动命令扩展或取消 region；`C-w` / `M-w` 作用于 region。
- motion 的语义类别：字符、词、行首 / 行尾、buffer 首 / 尾、前 / 后行、翻页。
- kill ring 的基本行为：`C-k` / `C-w` / `M-w` / `C-y`；连续 kill 的追加规则。
- undo 的命令边界：每个命令一个 undo 边界；连续自插入合并；移动 / 其它命令打断合并；undo / redo 键位。
- buffer 模型：buffer 名、当前 buffer、dirty 标记、`C-x b` / `C-x k`。
- minibuffer：`M-x` 读取命令名，`C-g` 取消，`RET` 确认。
- 退出流程：`C-x C-c` 对有未保存 buffer 的确认。

### F2：尽量一致（允许平台 / 实现造成的差异，需登记）

- 词边界与段落边界的精确规则（Emacs 用 syntax table；我们用简化规则）。
- `C-n` / `C-p` 的 goal column 与软换行下的行为。
- 滚动行为：`C-v` / `M-v` / `C-l` 的定位细节。
- mark ring、`C-x C-x`（exchange-point-and-mark）、`C-u` 数字参数、按键重复。
- isearch（`C-s` / `C-r`）的增量搜索与高亮。
- mode line 的信息与格式（显示哪些字段可以不同，但 buffer 名 / dirty / 行列必须存在）。
- kill-line 在 buffer 末尾报 "End of buffer" 的细节。
- 编码与换行：v1 只保证 UTF-8 / LF，其它编码与 CRLF 后续处理。

### F3：明确不同（有意差异，必须登记并在需要时提示用户）

- **连续 yank 的 undo 分组**：Emacs 会把连续 `C-y` 合并成一个 undo 单元；我们目前会分成多个。
- **undo-in-region**：region 激活时 Emacs 只撤销 region 内变更；我们执行全局 undo。
- **undo 环 / undo tree**：不实现 Emacs 的 undo 环与 `undo-in-region`。
- **同一 buffer 多窗口**：不保证独立 point / 滚动位置（见 D9）。
- **外观**：不模拟 Emacs 的外观、字体、菜单、工具栏、滚动条。
- **elisp 与包**：不实现。
- **syntax table / text property / narrowing / indirect buffer**：不实现。

### F4：不做

- org-mode、dired、tramp、daemon / client / server；
- 多 frame、tab-bar、GUI 菜单栏与工具栏；
- 终端复用与 TTY 界面；
- Emacs 的包管理器与配置语言。

## 3. 键位解析约定

- `C-` = control；`M-` = meta / alt；`S-` = shift；`s-` = super。
  组合键写作 `C-M-_`、`C-x C-s` 等。
- GUI 上 `TAB` 与 `C-i` 是两个不同事件，但绑定到同一命令（Emacs 在 GUI 下也区分 `<tab>` 与 `C-i`，默认都指向缩进 / 自插入语义）。
- `RET` / `<return>` 与 `C-m` 绑到同一命令；`C-j` 保持 Emacs 的 `newline-and-indent` 语义。
- `backspace` 是 DEL，绑定 `delete-backward-char`；`C-h` 是独立前缀，不映射到 backspace。
- `ESC` 与 `M-` 的关系（ESC 作为 Meta 前缀、ESC ESC 取消前缀）留待键位细节讨论，见 "待决事项"。
- 未定义键的判定：当前 keymap 链上没有任何绑定，且当前没有可用的自插入路径 → 消费并报错，不落字符。
- 自插入判定：`key_char` 存在、无 control / super 修饰、不是正在等待前缀的状态。
  平台特定情况（AltGr、死键、IME 预编辑）在 `src/platform.rs` 统一处理。

## 4. v1 默认键位（草案）

待"键位引擎细节"讨论后定稿。命令名使用 Emacs 名称，因为 `M-x` 和 `C-h` 系列会直接暴露它们。

| 键 | 命令 | v1 |
|---|---|---|
| `C-f` / `C-b` / `C-p` / `C-n` | forward-char / backward-char / previous-line / next-line | 是 |
| `C-a` / `C-e` | beginning-of-line / end-of-line | 是 |
| `M-f` / `M-b` | forward-word / backward-word | 是 |
| `M-<` / `M->` | beginning-of-buffer / end-of-buffer | 是 |
| `C-v` / `M-v` | scroll-up / scroll-down | 是 |
| `C-d` / `DEL` | delete-char / delete-backward-char | 是 |
| `C-k` | kill-line | 是 |
| `C-w` / `M-w` | kill-region / kill-ring-save | 是 |
| `C-y` | yank | 是 |
| `M-y` | yank-pop | 延后 |
| `C-SPC` / `C-@` | set-mark-command | 是 |
| `C-x C-x` | exchange-point-and-mark | 延后 |
| `C-g` | keyboard-quit | 是 |
| `C-/` / `C-_` / `C-x u` | undo | 是 |
| `C-?` / `C-M-_` | undo-redo | 是 |
| `M-x` | execute-extended-command | 是 |
| `C-x C-f` / `C-x C-s` / `C-x C-w` | find-file / save-buffer / write-file | 是 |
| `C-x b` / `C-x k` | switch-to-buffer / kill-buffer | 是 |
| `C-x C-c` | save-buffers-kill-emacs | 是 |
| `C-s` / `C-r` | isearch-forward / isearch-backward | 延后 |
| `C-u` | universal-argument | 延后 / 视讨论 |
| `C-x 1/2/3/o` | window 管理 | 延后 |

## 5. undo 行为对照（实测）

| 场景 | Emacs 31.1 | gpui-kit | 结论 |
|---|---|---|---|
| 输入 `abcdef`，一次 undo | 全部删除 | Typing 事务合并 | 一致 |
| 输入 `abc`，移动，再输入 `def`，一次 undo | 只删 `def` | move_to 打断合并 | 一致 |
| 输入 `abc`，`C-b`，输入 `X`，undo | 只删 `X` | 位置不连续不合并 | 一致 |
| 连续 `DEL` 两次，undo | 一次恢复两个字符 | Backspace 相邻合并 | 一致 |
| 连续 `C-d` 两次，undo | 一次恢复两个字符 | DeleteForward 相邻合并 | 一致 |
| `C-k` 两次，undo | 只恢复第二刀（两个 undo 单元） | 每次 replace 是 Atomic | 一致 |
| `C-k` 后 `C-y` 两次，undo | 两次 yank 一起撤销 | 每次 insert 是 Atomic | **不一致** |
| 输入 `abc` + `RET` + `def`，undo | 只删 `def` | RET 是 Atomic，之后重新开始 | 一致 |
| undo 到空后再 undo | 报 "No further undo information"，不 redo | undo 栈空；redo 是独立 action | 一致 |
| undo 后移动再 redo | 可以 redo | redo 栈只在编辑时清空 | 一致 |
| undo 后输入新内容再 redo | redo 链被清空 | push_batch 清空 redo | 一致 |
| region 激活时 undo | 只撤销 region 内变更 | 全局撤销 | **不一致** |
| undo 后的 mark | 位置保留但 deactivate | 恢复内部 selection；我们的 mark 自己清 | 由我们实现 |
| 历史容量 | 基本无上限 | 1000 个事务 / 每事务 1000 changes | 差异，可接受 |

## 6. 待决事项（键位细节讨论时定）

- `ESC` / `M-` / `C-g` 的完整语义；`ESC ESC` 是否映射 `keyboard-quit`。
- `C-u` 数字参数的实现范围（仅 `C-u N`，还是包含 `M-N`、`C-u C-u`）。
- 未定义键的 echo 文案与超时；prefix 的显示方式（echo area 显示到哪一步）。
- `C-?` 在 Wayland 下的键名（可能是 `ctrl-shift-slash`）。
- 是否支持 `C-x` 前缀的超时取消；Emacs 默认无超时。
- 系统剪贴板与 kill ring 的同步策略。
