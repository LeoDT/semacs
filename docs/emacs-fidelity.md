# Emacs 保真度

本文定义"像 Emacs 到什么程度"，是本项目**最重要的约束**。实现中出现"要不要和 Emacs 一样"的争议时，先在这里判定级别。

参照实现：GNU Emacs 31.1。对照条件与证据要求见第 5 节；旧实测描述须按固定脚本复核，
不得视为 semacs 已验证的结果。平台差异（字体、滚动、IME）单独讨论。

分级定义行为要求，不等于发布范围。v1 范围以 [requirements.md](requirements.md) 为准；
isearch、`C-u`、mark ring 等即使列为 F2，也在 v1 之后评估。新增 F3 差异须经确认，不能用来绕过失败的 spike。

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
- `C-g` = `keyboard-quit`：取消前缀、取消 minibuffer、停用 region、echo "Quit"；不删除已有 mark 位置。
- 未定义键：消费按键、不插入字符，echo 形如 `<键序列> is undefined`，不静默吞掉。
- 自插入：普通可打印字符与 Shift 产生的可打印字符插入自身；普通 control / Meta / super 组合不自插入。
  AltGr、死键和 IME 由平台输入分类处理，不能套用普通命令修饰键规则。
- point / mark / region 的基本语义：`C-SPC` 设 mark；mark 位置与 region 激活状态独立；
  移动、编辑、取消和 undo 后的状态有独立断言；`C-w` / `M-w` 作用于 region。
- motion 的语义类别：字符、词、行首 / 行尾、buffer 首 / 尾、前 / 后行、翻页。
- kill ring 的基本行为：`C-k` / `C-w` / `M-w` / `C-y`；连续 kill 的追加规则。
- undo 的命令边界：原子编辑按命令撤销，连续自插入与删除的合并规则按第 5 节固定脚本复核；
  移动 / 其它命令打断合并，不能把“每条命令一个边界”解释成无条件阻止所有合并；undo / redo 键位一致。
- buffer 模型：buffer 名、当前 buffer、dirty 标记、`C-x b` / `C-x k`。
- minibuffer：`M-x` 读取命令名，`C-g` 取消，`RET` 确认。
- 文件安全与退出：保存失败不清 dirty；保存后 undo 正确反映是否偏离保存内容；
  kill dirty buffer、`C-x C-c` 与窗口关闭均防止静默丢失未保存内容，取消后保留 buffer 与焦点。

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

- **连续 yank 的 undo 分组**：已接受 semacs 将连续 `C-y` 分成多个 undo 单元；
  原记录中的 Emacs 合并条件仍须按第 5 节固定脚本复核，不将其表述为无条件行为。
- **undo-in-region**：region 激活时 Emacs 只撤销 region 内变更；我们执行全局 undo。
- **undo 环 / undo tree**：不实现 Emacs 的 undo 环与 `undo-in-region`。
- **窗口拆分**：v1 不提供 split；后续不同 buffer split 需先明确键位语义，同 buffer 多视图仍不承诺（见 D14）。
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
- `RET` / `<return>` 与 `C-m` 绑到同一命令；本机 Emacs 31.1 `-Q` 的 `fundamental-mode` 中，
  `RET` 为 `newline`，`C-j` 为 `electric-newline-and-maybe-indent`，`TAB` 为 `indent-for-tab-command`。
  对照时记录 electric indent 等选项，不假定 `C-j` 始终绑定 `newline-and-indent`，也不把 scratch 的绑定当作全局规则。
- `backspace` 是 DEL，绑定 `delete-backward-char`；`C-h` 是独立前缀，不映射到 backspace。
- `ESC` 与 `M-` 的关系（ESC 作为 Meta 前缀、ESC ESC 取消前缀）留待键位细节讨论，见 "待决事项"。
- 未定义键的判定：当前 keymap 链上没有任何绑定，且当前没有可用的自插入路径 → 消费并报错，不落字符。
- 自插入判定：空前缀、无绑定、可打印且无 control / Meta / super；Shift 不阻止文本输入。
  `key_char` 存在本身不是充分条件。AltGr 必须经平台确认是文本输入；死键与 IME 预编辑不能误判为 undefined。
- 系统保留键由平台 / 焦点路由显式处理；编辑器目标内的其它未绑定命令键统一报 undefined，不设静默 `Ignored` 分支。
- undefined 测试序列必须先检查参照 Emacs 与 semacs 的当前 keymap。`C-x a` 在 Emacs 31.1 是 abbreviation 前缀，
  不能用作双方 undefined 的示例。semacs 可不实现该功能，但应记录为范围差异。

## 4. v1 默认键位（草案）

待"键位引擎细节"讨论后定稿。命令名使用 Emacs 名称，因为 `M-x` 和 `C-h` 系列会直接暴露它们。

| 键 | 命令 | v1 |
|---|---|---|
| `C-f` / `C-b` / `C-p` / `C-n` | forward-char / backward-char / previous-line / next-line | 是 |
| `C-a` / `C-e` | beginning-of-line / end-of-line | 是 |
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
| `C-x C-c` | save-buffers-kill-emacs | 是 |
| `C-s` / `C-r` | isearch-forward / isearch-backward | 延后 |
| `C-u` | universal-argument | v1 后评估 |
| `C-x 1/2/3/o` | window 管理 | 延后 |

## 5. 对照基准与 undo 验收矩阵

### 固定基准

- 使用 GNU Emacs 31.1、`emacs -Q`、专用 `fundamental-mode` buffer，显式启用 undo 与 transient mark。
- 每个用例记录初始文本、point / mark / 激活状态、kill ring、输入序列、命令边界和影响行为的选项；
  涉及按键时记录 GUI / batch 驱动方式，不能把直接调用函数等同于真实 command loop。
- 测试脚本与预期结果在 spike 阶段固化到验证工程；记录 Emacs 版本、依赖解析版本与 `Cargo.lock`、
  平台、运行命令和实际结果。临时脚本放 `.pi/tmp/`，验证通过的回归用例随正式工程迁移。
- semacs 必须经 interceptor → 命令 → Buffer 真实路径验证，不能拿底座原生 action 的结果代替。
- undefined 用例先断言双方均无绑定；模式特定绑定另设用例。IME 还需 Wayland 手工验证。

### 当前证据边界

旧表未附可复现脚本，也混用了底座原生动作与自有编辑入口。下表替代旧的“一致”结论：
所有 semacs 结果均待 S3 / S4 验证，Emacs 分组细节也须先固定条件复测。

参考源码检查表明：公开 `insert` / `replace` 请求 Atomic 意图；原生 Backspace / Delete 使用各自可合并的意图；
`set_selected_range` 经 `move_to` 打断合并。这些事实不能推出 semacs 自有删除与 Emacs 一致。

| 场景 | 对照要求 / 待复核点 | semacs 验证要求 |
|---|---|---|
| 连续输入 `abcdef`，undo | 固定连续输入的合并条件 | 不因观测或 caret 同步拆散输入 |
| 输入 `abc`，移动，再输入 `def`，undo | 移动打断合并 | 撤销后 point / 文本正确 |
| 连续 `DEL` / 连续 `C-d`，undo | 复核旧记录“一次恢复两个字符”的条件 | 走自有删除入口，不以底座原生删除替代 |
| `C-k` 两次，undo | 固定每次 kill 的命令边界 | kill ring 追加不等于 undo 合并 |
| `C-k` 后 `C-y` 两次，undo | 复核 Emacs 连续 yank 的分组条件 | 允许已登记的 yank 分组差异 |
| 输入 `abc` + `RET` + `def`，undo | 换行与连续输入的边界 | 验证自有 newline 命令与输入路径 |
| undo 到无历史后继续 undo | 固定错误回显与重复 undo 行为 | 不静默失败、不意外 redo |
| undo 后移动 / 输入，再 redo | 分别记录参照行为；undo 环差异单独登记 | 记录线性历史的 redo 可用性，不能宣称所有分支与 Emacs 相同 |
| region 激活时 undo | 对照 region 内撤销行为 | 已接受全局 undo；不得破坏 mark 模型 |
| undo / redo 后的 mark | 保留位置与跟随，激活状态单独断言 | 不能用清 mark 冒充 deactivate |
| 连按 undo；undo 后立即输入 / 切换 buffer / 进入 minibuffer | 记录操作序列及目标 | 完成顺序、焦点、文本与 caret 一致 |
| 保存 → 编辑 → undo；保存 → undo / redo | 固定保存点与 dirty 的可观察结果 | dirty 与保存内容一致，保存失败不更新保存点 |
| IME 更新 → 提交 / 取消 → undo | 记录预编辑替换范围与最终状态 | 取消不留下净文本 / mark 偏移或错误 dirty |
| 历史容量 | Emacs 受 undo 限额与清理影响，并非无上限 | 记录底座实际限额；依赖升级时复核，不以此声称等价 |

S3 / S4 未通过时，不将对应行改为 F3 以绕过验收；处理流程见 D14。

## 6. 待决事项（键位细节讨论时定）

- `ESC` / `M-` / `C-g` 的完整语义；`ESC ESC` 是否映射 `keyboard-quit`。
- v1 之后的 `C-u` 数字参数范围（仅 `C-u N`，还是包含 `M-N`、`C-u C-u`）。
- 未定义键的 echo 文案与超时；prefix 的显示方式（echo area 显示到哪一步）。
- `C-?` 在 Wayland 下的键名（可能是 `ctrl-shift-slash`）；`RET` / `TAB` / `C-j` 在基准选项下的具体命令与基础缩进行为，须在 M1 前固定。
- 前缀默认无超时；若拟增加超时取消，须另行确认差异。
- 系统剪贴板与 kill ring 的同步策略。
