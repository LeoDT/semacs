# Emacs 保真度

本文是编辑行为的唯一契约，以 GNU Emacs 31.1 为参照，不表示产品已实现或通过验收。
发布范围见 [需求与范围](requirements.md)；分级不自动把延后能力纳入 v1。新增 F3 差异须明确批准。

## 1. 判定方法

比较同样键序列触发的命令、可观察状态和交互流程：文本、point、mark / active、kill ring、modified、echo、mode line、前缀等待与 minibuffer。
内部数据结构、绘制方式和算法无需相同。

## 2. 保真度分级

### F1：必须一致（不一致即 bug）

- 键序列与命令名、前缀等待、取消、undefined 与自插入，见 [键位解析约定](#3-键位解析约定)。
- 字符 / 词 / 行 / buffer 首尾移动及翻页的基本语义；Emacs 字符运动不等于 grapheme 显示边界。
- point / mark / region、kill / yank、历史分组及保存点，见 [编辑行为契约](#6-编辑行为契约)。
- A → B → A 时文本、point / mark、滚动、history 与显示状态不串用；active 按参照命令结果判断。
- 正文与 minibuffer 共用基础 motion、编辑、mark、kill / yank 语义及全局 kill ring；undo history 独立，prompt 不可编辑。
- `M-x` 读命令名、`RET` 确认、`C-g` 取消会话并回显 `Quit`；命令名、路径与 buffer 名提供基础补全。
- IME、文件与退出遵循第 6 节的已批准策略。
- echo area 展示 pending 前缀、命令消息与错误。

### F2：尽量一致（允许平台 / 实现差异，须登记）

- 词 / 段落边界的精确规则；可以明确登记简化规则，不承诺完整 syntax table。
- `C-n` / `C-p` 的 goal column 与软换行行为，`C-v` / `M-v` / `C-l` 的精确滚动定位。
- mark ring、`C-x C-x`、`C-u`、按键重复及 isearch 的保真度；是否实施仍由发布范围决定。
- mode line 格式可不同，但至少提供 buffer 名、dirty 与行列。
- `kill-line` 在 EOB 的提示细节，以及第 6 节批准的 IME / 文件格式策略；与 Emacs 的差异须在用户可见说明中登记。

### F3：明确不同（须登记，必要时提示用户）

- region 激活时仍执行全局 undo；不实现 undo-in-region 或 Emacs undo 环 / undo tree。
- v1 不提供 split；不承诺同 buffer 多 view。
- 不模拟 Emacs 外观、字体、菜单、工具栏与滚动条。
- 不实现 elisp、syntax table、text property、narrowing 或 indirect buffer。

### F4：不做

不做的产品能力统一见 [非目标](requirements.md#非目标f4)，与仅延后到 v1 后的能力区分，不另维护一份清单。

### M1 已登记的 F2 motion 差异

- 词由连续 Unicode 字母 / 数字组成；其余字符作为分隔符，不实现 syntax table 或按文字系统细分词。下划线、组合附加符号和 emoji 均为分隔符；例如 `M-f` 在 `éclair` 中先停在 `e` 后，在 `🙂last` 中直接到 `last` 后，与 Emacs 的精确边界不同。
- `C-n` / `C-p` 按逻辑行移动，不按软换行显示行；连续上下行共用临时目标列，其他命令、指针定位和平台输入提交打断。列宽复用基础缩进的 Unicode 宽度与 tab-width=8；目标列落在 TAB / 宽字符内时越过该字符，不实现行尾特殊 goal 或像素目标列。
- 翻页按完整可见显示行数减两行移动（至少一行）；point 仍在新页内则保留，否则移到最近可见行的行首。末页 / 首页先移到 buffer 边缘，再次同向翻页报边界错误；不承诺 Emacs 的精确窗口边距与列定位。
- `C-l` 连续执行按居中 → 顶部 → 底部循环，其他命令打断；按像素定位，允许 EOB 后留白，不承诺 Emacs 的整行取整和滚动边距。
- 翻页只使用当前文本 / 布局设置兼容且完整的 wrap index；尚未就绪时提示重试，不同步扫描全文、不排队自动重放。recenter 可等当前 point 的兼容显示行就绪后定位。

## 3. 键位解析约定

- `C-` = control；`M-` = meta / alt；`S-` = shift；`s-` = super；组合写作 `C-M-_`、`C-x C-s`。
- 使用分层 keymap；前缀无超时，命令绑定与前缀冲突须被发现。系统保留键由平台 / 焦点路由处理。
- 顶层 `C-g` 停用 region、保留 mark，回显 `Quit`；前缀中的 `C-g` 依当前 keymap 解析。`C-x C-g` 为 undefined，清空 pending，保留 mark / active，不执行顶层 quit 副作用。
- 无绑定且无自插入路径的编辑目标按键被消费，回显 `<键序列> is undefined`，不插入字符。
- 自插入只在空前缀、无绑定、可打印且无 control / Meta / super 时成立；Shift 不阻止输入，`key_char` 存在本身不充分。AltGr、死键与 IME 须按平台输入分类，不可误报 undefined。
- 默认在 point 输入，不因 region 激活而替换 region，不启用 delete-selection-mode。复制 / 粘贴走 `M-w` / `C-y`，不提供另一套 CUA 编辑路径。
- GUI `TAB` / `<tab>` 与 `C-i` 区分；`RET` / `<return>` 与 `C-m` 绑定同一命令。参照 `fundamental-mode` 中 `RET` 为 `newline`，`C-j` 为 `electric-newline-and-maybe-indent`，`TAB` 为 `indent-for-tab-command`；须固定 electric indent 等选项，不用 scratch 绑定代替全局规则。
- backspace 是 DEL，绑定 `delete-backward-char`；`C-h` 是独立前缀，不映射到 backspace。
- 支持 Alt / Meta 与传统 `ESC` 前缀，前缀无超时；正文 `ESC` 不等同 `C-g`，`ESC ESC` 等序列按 Emacs 31.1 参照核对，不从确认框行为推导。
- M1 基础缩进以 `emacs -Q` 的 `fundamental-mode` 为基线，核对并固定相关选项；不包含语言智能缩进或格式化。

## 4. v1 默认键位（草案）

使用 Emacs 命令名。M1 编辑命令的标识与默认绑定已固定于 [注册表](../src/command.rs)，并与 [Emacs 绑定查询](../tests/emacs/m1-bindings.tsv) 逐项核对；注册不表示执行已接入或通过验收。下表仍包含 M2 与延后能力。

| 键 | 命令 | v1 |
|---|---|---|
| `C-f` / `C-b` / `C-p` / `C-n` | forward-char / backward-char / previous-line / next-line | 是 |
| `C-a` / `C-e` | move-beginning-of-line / move-end-of-line | 是 |
| `M-f` / `M-b` | forward-word / backward-word | 是 |
| `M-<` / `M->` | beginning-of-buffer / end-of-buffer | 是 |
| `C-v` / `M-v` | scroll-up-command / scroll-down-command | 是 |
| `C-d` / `DEL` / GUI Delete | delete-char / delete-backward-char / delete-forward-char | 是 |
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

## 5. 对照基准与验收

- 固定 GNU Emacs 31.1、`emacs -Q`、专用 `fundamental-mode` buffer，显式启用 undo 与 transient mark。
- 记录初始文本、point / mark / active、kill ring、命令边界与影响行为的选项。真实 command loop、GUI / pty 驱动和直接函数调用须区分。
- 外部观察与采集陷阱见 [Emacs 对照基准](../tests/emacs/README.md)，不把参照结果当作 semacs 通过记录。
- undefined 用例须先确认双方均无绑定；`C-x a` 在 Emacs 31.1 是 abbreviation 前缀，不能用作双方均 undefined 的例子。
- 产品验收走正式 GPUI 输入、命令路由和 Edit 入口，直接 IME 回调路径单独核验；组件单测不能替代真实平台 IME / 候选窗验证。
- 对无历史 / 重复 undo、undo 后移动或输入再 redo、换行与输入分组、history 容量与清理策略保留参照；不声称自有线性历史的所有分支都等同 Emacs。
- 阶段验收范围见 [实施计划](plan.md)，性能约束见 [性能预算](performance.md)。

## 6. 编辑行为契约

### 历史与保存点

- 连续同类自插入、连续后删、连续前删分别合并；移动、换行与其他命令打断合并。
- 连续 `kill-line` 各自成组，连续 yank 也各自成组；kill ring 追加不等于历史合并。
- undo / redo 不与输入合并；undo 后输入开新组并清空 redo。
- undo 恢复组开始前的 point；redo 回到组内最早变更起点，不是组末。
- 回到保存点为 clean；已固定的保存后 undo 越过保存点场景仍 clean，再输入为 dirty。磁盘内容单独验证，不以当前文本等于磁盘重定义 modified。
- 历史回放不登记为新的用户编辑；各正文 buffer 和 minibuffer 的历史独立。

### mark 与 region

- mark 位置与 active 独立；停用 region 不删除 mark。
- mark 为 insertion type nil 的 marker：在其前插入则后移，在其位置或之后插入不动。
- 删除覆盖 mark（包括删除末端）时坍缩到删除起点；undo 仅在 mark 仍位于该起点时恢复原偏移，否则只按文本位移调整；redo 再按删除规则处理。
- 纯 marked 更新 / 丢弃不调整 mark；平台已提交替换和最终输入提交按已提交编辑调整。region 高亮按 committed point / mark 投影到当前显示，跳过仍 marked 的预编辑文字；它不改变平台输入的 caret / selected range，也不启用 delete-selection。
- 输入提交、undo / redo 与顶层 `C-g` 停用 region，但保留 mark。

### kill ring

- `C-k` / `C-w` / `M-w` / `C-y` 遵循参照语义；`C-w` / `M-w` 在 mark 已设置时使用 point / mark 范围，不要求 active；空 region 可写入空 ring 条目但不创建文本编辑。kill / copy 停用 region，保留 mark。
- 连续 kill 按应用级命令链追加；正向追加、反向 region 前置。非 kill 命令、undefined、指针定位、输入会话结束及平台输入提交打断连续 kill；等待前缀本身不是命令。`M-w` 可续接前一条 kill，但自身不构成下一条 kill 的连续前驱。
- `kill-line` 在 EOB 回显 `End of buffer`，不建历史组。
- 只读 buffer 的 kill 按参照复制到 ring 后报告只读错误，不修改文本 / modified；`C-k` 此时还会把 point 移到被复制范围末端。`M-w` 在只读 buffer 可正常复制；只读 yank 在改变 mark 前拒绝。
- yank 在插入起点设置 mark，回显 `Mark set`；非空插入停用 region，不替换原 region。空 ring 报错但仍在 point 设置 mark；空字符串 yank 无文本编辑，不主动停用已有 active。
- 正文与 minibuffer 共用应用内 kill ring，不读写系统剪贴板。默认最多保留 120 条、不去重，与固定 Emacs 31.1 默认值一致；连续追加仍只占一条。

### 只读前缀与 field

- 只读文本范围与整个 buffer 只读分开判断。M1 先支持固定只读前缀，不引入完整 text-property 或任意可变 field 系统。
- minibuffer 的 prompt 是 committed 文本中的只读前缀，point / mark 可以进入，region / copy 可以包含；不使用外置 label 替代。在前缀内插入或删除 / 替换其文字均被拒绝，在前缀末端插入允许。统一 Edit 校验及平台预编辑 / 提交路径都执行保护，undo / redo 不能绕过。
- 字段边界独立于只读属性，由共享命令层使用。目前支持在不可变前缀中固定的分界：C-a 从输入字段移到当前逻辑行首，但不进入 prompt；已在 prompt 内时仍可移到该字段内的行首。C-k 的范围止于所在字段的行尾 / 末端。字符移动和 buffer 跳转不统一截断到输入区；C-e 仍到逻辑行末，不能把所有 motion 都限制在字段内。
- 删除 prompt 的 kill 先复制，再报告只读文本错误；C-k 还移到复制范围末端。yank 的整个 buffer 只读检查发生在设置 mark 前；仅插入位置为只读文本时，非空 yank 设置 mark 后因插入受保护而失败。空 ring 和空字符串 yank 保留既定的 mark 副作用，不创建文本编辑。
- prompt / 初始输入在创建目标时初始化，不属于用户 undo 组；minibuffer 的 modified 不代表文件未保存。正文与 minibuffer 的 history、point / mark、输入会话和布局独立，kill ring 仍应用内共享。
- M1 最小 minibuffer 仅是可挂载、聚焦的编辑组件，沿用 M1 编辑 keymap；RET / C-j 仍执行编辑换行，C-g 仍停用 region 并回显 Quit，不假装完成读入确认 / 取消。M-x、读入专用绑定、补全及完整会话由 M2 接入。

### IME 与生命周期

- 目标存在未提交预编辑时，拒绝编辑器命令和前缀，回显 `Input method composition is active; finish it first`。IME 自己消费的候选操作不算编辑器命令，组合完成后用户重新触发命令。
- 纯 marked 更新 / 丢弃不改变 committed、point 或 modified，也不清除已有 dirty；平台显式正文替换立即作为已提交变更，可置 dirty，取消不回滚。
- 部分 marked 被替换后，未再 marked 的文字成为 committed；最终提交仅插入剩余 marked 文字一次。unmark 保留文字，与丢弃 marked 不同。
- 窗口失焦 / 关闭丢弃仍 marked 的文字，不强制提交或保存；保存只读取 committed。
- 输入绑定原目标和会话，切换 / 关闭 / 重开后的迟到回调不写入新目标；pending 前缀与只读约束不能被直接文本输入绕过。
- caret、命中及候选坐标须对应兼容布局；临时 / 最终可见布局就绪后刷新平台几何。同长度文本更新也不能复用不兼容的旧几何。

### 文件与退出

- 支持 UTF-8 有 / 无 BOM、统一 LF / CRLF；内部无 BOM、使用 LF，保存恢复原始格式，新换行沿用 buffer 格式。
- 不自动增删末尾换行或重复 BOM；未编辑文件解码 / 编码往返字节一致，读取和格式适配不产生历史或 dirty。
- 明确拒绝无效 UTF-8、UTF-16 / UTF-32 BOM、含 NUL、混合换行及裸 CR，不猜测编码或静默归一化；扩展支持先决定保留 / 转换策略。
- 保存失败不清 dirty、不更新文件身份或保存点、不破坏原文件；覆盖确认的具体流程和文案仍待确认。
- kill dirty buffer、退出与窗口关闭按 modified 确认；取消后保留正文与焦点。组合状态按上一节处理，已有 dirty 不因丢弃预编辑消失。
- 外围确认框安全默认 Cancel；Escape / `C-g` 取消，Tab / Shift-Tab / 左右键选择，Enter / Space 确认。
