# Emacs 保真度

本文定义 semacs 的 Emacs 行为目标。GNU Emacs 31.1 是行为参照；此处的要求是验收标准，不表示 semacs 已实现或验证对应编辑能力。

分级定义行为要求，不等于发布范围。v1 范围见 [requirements.md](requirements.md)；isearch、`C-u`、mark ring 等即使列为 F2，也不自动纳入 v1。新增 F3 差异须明确批准。

## 1. 判定方法

按以下顺序比较：

1. **键序列相同**：同样的按键触发同语义命令。
2. **可观察结果相同**：buffer 文本、point、mark、kill ring、echo area 与 mode line 状态符合基准。
3. **交互流程相同**：前缀等待、`C-g`、未定义键与 minibuffer 流程符合基准。
4. **内部实现不构成行为要求**：数据结构和绘制方式可以不同。

## 2. 保真度分级

### F1：必须一致（不一致即 bug）

- 键序列与命令名：`C-f` = `forward-char`，`C-k` = `kill-line` 等。
- 前缀键：`C-x`、`C-c`、`M-`、`C-h` 的等待与完成行为。
- 顶层 `C-g` = `keyboard-quit`：停用 region、echo `Quit`，保留 mark 位置；minibuffer 中取消会话并回显 `Quit`。前缀中 `C-g` 依据当前 keymap 解析；`C-x C-g` 为 undefined，清空前缀但不执行顶层 quit 副作用，mark 与 active 状态不变。
- 未定义键：消费按键、不插入字符，并在 echo area 回显 `<键序列> is undefined`。
- 自插入：普通可打印字符与 Shift 产生的可打印字符插入自身；普通 control / Meta / super 组合不自插入。AltGr、死键和 IME 依平台输入分类处理。
- point / mark / region：`C-SPC` 设 mark；mark 位置与 region 激活状态独立；移动、编辑、取消及 undo 后分别检查状态；`C-w` / `M-w` 作用于 region。
- motion 覆盖字符、词、行首/行尾、buffer 首尾、前后行与翻页语义。
- kill ring：`C-k` / `C-w` / `M-w` / `C-y`、连续 kill 追加规则；正文 buffer 与 minibuffer 共用 kill ring。
- undo 命令边界：原子编辑、连续自插入与删除按固定基准分组；移动及其他命令打断合并；undo / redo 键位一致。不可将“按命令撤销”解释为阻止 Emacs 的合法连续编辑合并。
- buffer：buffer 名、当前 buffer、dirty、`C-x b` / `C-x k`；A → B → A 时 point / mark 不丢失、不串用，激活状态依基准命令结果判断。
- minibuffer：`M-x` 读命令名、`C-g` 取消、`RET` 确认；基础 motion / 编辑 / mark / kill / yank 与正文语义一致，undo history 独立，prompt 不可编辑。
- IME 与命令交错：不保存临时预编辑、不把迟到输入写入新目标；pending 前缀期间文本提交不可绕过按键解析。D17 在组合中拒绝命令/前缀；D19 在关闭/失焦时取消未提交预编辑。
- 文件与退出：保存失败不清 dirty；modified/save-point 遵循 D16 基准，不用当前文本与磁盘内容比较替代。kill dirty buffer、`C-x C-c` 与窗口关闭按 modified 状态确认；取消后保留 buffer 与焦点。

### F2：尽量一致（允许平台/实现差异，须登记）

- 词边界与段落边界的精确规则（Emacs 使用 syntax table，可采用明确记录的简化规则）。
- `C-n` / `C-p` 的 goal column 与软换行行为。
- D17 / D19 所定 IME 策略属于用户批准的行为；须在用户可见说明中登记。
- `C-v` / `M-v` / `C-l` 的精确滚动定位。
- mark ring、`C-x C-x`、`C-u` 数字参数、按键重复。
- isearch（`C-s` / `C-r`）的增量搜索与高亮。
- mode line 字段格式可不同，但须有 buffer 名、dirty 与行列信息。
- `kill-line` 在 buffer 末尾的 `End of buffer` 提示细节。
- D20 文件格式：v1 支持 UTF-8 有/无 BOM、统一 LF/CRLF，内部 LF、保存恢复原格式；无效 UTF-8 与不支持编码明确报错；混合换行当前明确拒绝，扩展支持须先确认保留 / 转换策略。

### F3：明确不同（须登记，必要时提示用户）

- **undo-in-region**：region 激活时执行全局 undo，不限于 region 内变更。
- **undo 环 / undo tree**：不实现 Emacs undo 环与 undo-in-region。
- **窗口拆分**：v1 不提供 split；后续评估需先明确不同 buffer split 的键位语义，同 buffer 多 view 不在 v1 承诺内。
- **外观**：不模拟 Emacs 外观、字体、菜单、工具栏与滚动条。
- **elisp 与包**：不实现。
- **syntax table / text property / narrowing / indirect buffer**：不实现。

### F4：不做

- org-mode、dired、tramp、daemon / client / server；
- 多 frame、tab-bar、GUI 菜单栏与工具栏；
- 终端复用与 TTY 界面；
- Emacs 包管理器与配置语言。

## 3. 键位解析约定

- `C-` = control；`M-` = meta / alt；`S-` = shift；`s-` = super。组合键写作 `C-M-_`、`C-x C-s`。
- GUI 上 `TAB` 与 `C-i` 是不同事件；Emacs GUI 基准区分 `<tab>` 与 `C-i`，并分别记录绑定。
- `RET` / `<return>` 与 `C-m` 绑定同一命令。GNU Emacs 31.1 `-Q` 的 `fundamental-mode` 中，`RET` 为 `newline`，`C-j` 为 `electric-newline-and-maybe-indent`，`TAB` 为 `indent-for-tab-command`。对照记录 electric indent 等选项，不以 scratch 绑定代替全局规则。
- `backspace` 是 DEL，绑定 `delete-backward-char`；`C-h` 是独立前缀，不映射到 backspace。
- `ESC` / `M-` 的完整交互及 `ESC ESC` 是否映射 `keyboard-quit` 仍待键位细节确认。
- undefined：当前 keymap 链无绑定且无可用自插入路径时，消费并报错，不落字符。自插入只在空前缀、无绑定、可打印且无 control / Meta / super 时成立；Shift 不阻止文本输入。`key_char` 存在本身不充分；AltGr 须经平台确认，死键与 IME 预编辑不可误判为 undefined。
- 系统保留键由平台/焦点路由显式处理；编辑目标内其他未绑定命令键报 undefined。
- undefined 用例须先确认 Emacs 31.1 与 semacs 当前 keymap 均无绑定。`C-x a` 在 Emacs 31.1 是 abbreviation 前缀，不可作双方均 undefined 的例子。

## 4. v1 默认键位（草案）

命令名使用 Emacs 名称。正式实现前固定完整表，并在 M1–M2 实现时逐项核对；M3 汇总验收。

| 键 | 命令 | v1 |
|---|---|---|
| `C-f` / `C-b` / `C-p` / `C-n` | forward-char / backward-char / previous-line / next-line | 是 |
| `C-a` / `C-e` | move-beginning-of-line / move-end-of-line | 是 |
| `M-f` / `M-b` | forward-word / backward-word | 是 |
| `M-<` / `M->` | beginning-of-buffer / end-of-buffer | 是 |
| `C-v` / `M-v` | scroll-up-command / scroll-down-command | 是 |
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
| `C-x C-c` | save-buffers-kill-terminal | 是 |
| `C-s` / `C-r` | isearch-forward / isearch-backward | 延后 |
| `C-u` | universal-argument | v1 后评估 |
| `C-x 1/2/3/o` | window 管理 | 延后 |

GNU Emacs 31.1 `-Q` / `fundamental-mode` 的 `C-a`、`C-e`、`C-x C-c` 命令名已有核对记录。基准导出脚本存在不等于完整键位表已核对；必须在 M1–M3 按正式注册表逐项完成核验。

## 5. 对照基准与验收

### 固定基准

- 使用 GNU Emacs 31.1、`emacs -Q`、专用 `fundamental-mode` buffer，显式启用 undo 与 transient mark。
- 每个用例记录初始文本、point / mark / 激活状态、kill ring、输入序列、命令边界与影响行为的选项；涉及按键时记录 GUI / batch 驱动方式，不能将直接调用函数等同于真实 command loop。
- 独立 Emacs 基准用例与脚本集中于 `tests/emacs/`。记录 Emacs 版本、平台、运行命令和实际结果；脚本覆盖不等同于完整键位表核对。涉及用户真实环境的手工验收须事先征求同意。
- semacs 的行为验收须经真实 GPUI 平台输入、命令路由与编辑入口；组件单测不能代替端到端输入验证。IME 直接输入路径须单独验证目标归属、预编辑/提交/取消及 D17/D19。
- undefined 用例先断言双方均无绑定；模式特定绑定单独记录。IME 在 Wayland 验收环境验证。

### Emacs undo / 编辑基准矩阵

以下均为待执行的产品验收要求；不得据此宣称 semacs 已实现对应行为。R1 验收输入、预编辑、提交、布局几何及性能的最小切片；完整 undo/history 算法在 M1 实现；M3 完成全表 fidelity 验收。

| 场景 | Emacs 基准与 semacs 验收要求 |
|---|---|
| 连续输入 `abcdef` 后 undo | 固定连续输入的合并条件；检查恢复文本与 point |
| 输入 `abc`、移动、再输入 `def` 后 undo | 移动打断合并；检查分组与恢复位置 |
| 连续 `DEL` / `C-d` | 固定连续删除的 undo 分组 |
| `C-k` 两次后 undo | 每次 kill 的命令边界；kill ring 追加不等于 undo 合并 |
| `C-k` 后连续 `C-y` | 每次 yank 为独立单元 |
| 输入 `abc` + `RET` + `def` 后 undo | 固定换行与连续输入边界 |
| 无历史时继续 undo | 固定提示与重复 undo 行为，不静默失败或意外 redo |
| undo 后移动/输入，再 redo | 记录线性 history 行为，不声称所有分支等同 Emacs |
| region 激活时 undo | 记录 Emacs region 行为；semacs 按已登记 F3 执行全局 undo |
| mark 经插入、删除、undo/redo | mark 是 marker：在其前插入后移、在其位置插入不动、覆盖删除塌缩到起点；undo 仅在 mark 仍处于塌缩起点时恢复删除前位置；active 与位置独立 |
| undo / redo point | 依 Emacs 31.1 基准断言 point；记录分组起点与回放结果 |
| 保存、编辑、undo / redo | 保存后编辑并 undo 回保存点为 clean；已记录的跨越保存点 undo 场景仍 clean，再输入转 dirty；保存失败不更新保存点 |
| IME 更新后提交/取消，再 undo | 取消不留下已提交文本、mark 偏移或 dirty 变化；提交按实际替换范围应用一次 |
| 组合期间编辑、保存、切换目标、关闭 | D17 拒绝命令/前缀并提示；D19 关闭/失焦取消未提交预编辑；不保存临时文本、不改变已提交内容 |
| pending 前缀期间文本提交及迟到回调 | 文本不绕过解析；回调不可写入不同目标或新会话 |
| A 设置 point/mark → B 编辑 → A | point、mark、active 按 buffer 语义保持且不串用；不据此承诺多 view |
| 正文与 minibuffer 编辑 | 共用基础编辑语义及 kill ring；history 独立；prompt 不可编辑 |
| history 容量 | 记录自有实现的容量与清理策略，不以此声称等同 Emacs |
