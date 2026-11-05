# Emacs 保真度

本文是编辑行为的唯一契约，以 GNU Emacs 31.1 为参照，不表示产品已实现或通过验收。
发布范围见 [需求与范围](requirements.md)；分级不自动把延后能力纳入 v1。新增 F3 差异须明确批准。

## 1. 判定方法

比较同样键序列触发的命令、可观察状态和交互流程：文本、point、mark / active、kill ring、modified、echo、mode line、前缀等待与 minibuffer。
内部数据结构、绘制方式和算法无需相同。

## 2. 保真度分级

### F1：必须一致（不一致即 bug）

- 除已登记的 F3 默认键位差异外，键序列与命令名、前缀等待、取消、undefined 与自插入须一致，见 [键位解析约定](#3-键位解析约定)。
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
- 命令候选只来自本产品注册表，结果集与 Emacs obarray 不同；候选式读入采用 Vertico 式纵向选择与 fzf 风格评分匹配，见 [阶段 B](#候选式读入阶段-b)，不提供 `*Completions*` buffer。
- 普通确认走 minibuffer 查询会话；OS 窗口关闭等平台触发且无法可靠聚焦 minibuffer 的确认可使用窗口级确认，但须安全默认取消，并作为受控平台差异登记。

### F3：明确不同（须登记，必要时提示用户）

- 用户批准正文与 minibuffer 默认用 `C-t` 执行 `set-mark-command`，不是 Emacs 默认的 `transpose-chars`；保留 `C-@` 备用，取消编辑器的 `C-SPC` 绑定以留给平台 / 输入法切换。mark / region 的命令语义不变；未被平台消费而送入编辑器的 `C-SPC` 仍按普通无绑定键处理，不新增全局按键透传策略。
- region 激活时仍执行全局 undo；不实现 undo-in-region 或 Emacs undo 环 / undo tree。
- **v1 历史边界**：v1 不提供 split，也不承诺同 buffer 多 view。阶段 C 的现行行为以本文 [阶段 C：buffer 与 view](#阶段-cbuffer-与-view) 为准。
- 不模拟 Emacs 外观、字体、菜单、工具栏与滚动条。
- 不实现 elisp、syntax table、text property、narrowing 或 indirect buffer。

### F4：不做

不做的产品能力统一见 [非目标](requirements.md#非目标f4)，与仅延后到 v1 后的能力区分，不另维护一份清单。

### 已登记的 F2 motion 差异

- 词由连续 Unicode 字母 / 数字组成；其余字符作为分隔符，不实现 syntax table 或按文字系统细分词。下划线、组合附加符号和 emoji 均为分隔符；例如 `M-f` 在 `éclair` 中先停在 `e` 后，在 `🙂last` 中直接到 `last` 后，与 Emacs 的精确边界不同。
- `forward-paragraph` / `backward-paragraph` 按空行或仅含 ASCII 空格 / Tab 的逻辑行分隔段落，不实现 mode-specific 段落正则。向前停在当前段之后首个分隔行的行首，向后停在当前段之前最近分隔行的行首，而不是正文段首。从分隔行出发先越过当前连续空白行，再越过相应方向的一段正文，停在下一处分隔行；无目标时停在 buffer 首尾，重复不报边界错误。含空格 / Tab 的分隔行也停在行首；保留 mark / active，不建历史组，和词移动一样不按 minibuffer field 截断。当前没有默认键位，可通过 JS 配置绑定；不支持数值前缀参数。
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
- 基础缩进以 `emacs -Q` 的 `fundamental-mode` 为基线，核对并固定相关选项；不包含语言智能缩进或格式化。

## 4. v1 默认键位

使用 Emacs 命令名；命令标识、作用域和默认绑定以 [注册表](../src/command.rs) 为唯一实现来源，Emacs 参照见 [绑定查询](../tests/emacs/m1-bindings.tsv)。下表固定 v1 的键位范围，并列出相关延后能力，不维护执行接入或验收进度；阶段状态只见 [实施计划](plan.md)。

| 键 | 命令 | v1 |
|---|---|---|
| `C-f` / `C-b` / `C-p` / `C-n` | forward-char / backward-char / previous-line / next-line | 是 |
| `C-a` / `C-e` | move-beginning-of-line / move-end-of-line | 是 |
| `M-f` / `M-b` | forward-word / backward-word | 是 |
| `M-<` / `M->` | beginning-of-buffer / end-of-buffer | 是 |
| `C-v` / `M-v` | scroll-up-command / scroll-down-command | 是 |
| `C-d` / `DEL` / GUI Delete | delete-char / delete-backward-char / delete-forward-char | 是 |
| `M-d` / `M-DEL` | kill-word / backward-kill-word | 是（后续日常编辑增强） |
| `C-k` | kill-line | 是 |
| `C-w` / `M-w` | kill-region / kill-ring-save | 是 |
| `C-y` | yank | 是 |
| `M-y` | yank-pop | 延后 |
| `C-t` / `C-@` | set-mark-command | 是（已批准默认键位差异） |
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
- 连续 `kill-line` / `kill-word` / `backward-kill-word` 各自成组，连续 yank 也各自成组；kill ring 追加不等于历史合并。
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
- `kill-word` / `backward-kill-word` 使用与 `forward-word` / `backward-word` 相同的词边界，忽略 active region，删除 point 到目标词边界的内容；在 buffer 边缘范围为空时沿用空 region 的 ring / 无编辑语义，不报边界错误。只读时先复制并移动到目标边界，再报只读错误，不建历史组；跨 prompt 同样遵循只读保护，不单独截断到输入区。不支持数值前缀参数。
- 连续 kill 按应用级命令链追加；正向追加、反向 word / region 前置。非 kill 命令、undefined、指针定位、输入会话结束及平台输入提交打断连续 kill；等待前缀本身不是命令。`M-w` 可续接前一条 kill，但自身不构成下一条 kill 的连续前驱。
- `kill-line` 在 EOB 回显 `End of buffer`，不建历史组。
- 只读 buffer 的 kill 按参照复制到 ring 后报告只读错误，不修改文本 / modified；`C-k` 此时还会把 point 移到被复制范围末端。`M-w` 在只读 buffer 可正常复制；只读 yank 在改变 mark 前拒绝。
- yank 在插入起点设置 mark，回显 `Mark set`；非空插入停用 region，不替换原 region。空 ring 报错但仍在 point 设置 mark；空字符串 yank 无文本编辑，不主动停用已有 active。
- 正文与 minibuffer 共用应用内 kill ring，不读写系统剪贴板。默认最多保留 120 条、不去重，与固定 Emacs 31.1 默认值一致；连续追加仍只占一条。

### 只读前缀与 field

- 只读文本范围与整个 buffer 只读分开判断。支持固定只读前缀，不包含完整 text-property 或任意可变 field 系统。
- minibuffer 的 prompt 是 committed 文本中的只读前缀，point / mark 可以进入，region / copy 可以包含；不使用外置 label 替代。在前缀内插入或删除 / 替换其文字均被拒绝，在前缀末端插入允许。统一 Edit 校验及平台预编辑 / 提交路径都执行保护，undo / redo 不能绕过。
- 字段边界独立于只读属性，由共享命令层使用。目前支持在不可变前缀中固定的分界：C-a 从输入字段移到当前逻辑行首，但不进入 prompt；已在 prompt 内时仍可移到该字段内的行首。C-k 的范围止于所在字段的行尾 / 末端。字符移动和 buffer 跳转不统一截断到输入区；C-e 仍到逻辑行末，不能把所有 motion 都限制在字段内。
- 删除 prompt 的 kill 先复制，再报告只读文本错误；C-k 还移到复制范围末端。yank 的整个 buffer 只读检查发生在设置 mark 前；仅插入位置为只读文本时，非空 yank 设置 mark 后因插入受保护而失败。空 ring 和空字符串 yank 保留既定的 mark 副作用，不创建文本编辑。
- prompt / 初始输入在创建目标时初始化，不属于用户 undo 组；minibuffer 的 modified 不代表文件未保存。正文与 minibuffer 的 history、point / mark、输入会话和布局独立，kill ring 仍应用内共享。
- 读入 / 查询会话在 minibuffer 之上增加会话层：文本读入用 RET 确认、TAB 补全；单键查询直接作答，不要求 RET，也不回落到编辑 keymap。C-g 取消并回显 `Quit`；没有会话的裸挂载仍是普通编辑目标，沿用编辑 keymap。同一时间只允许一个会话，不做递归 minibuffer。确认与取消都先结束会话、恢复 active 正文的输入会话与焦点，保留 origin 的 point / mark / 滚动，再在 origin 上执行命令；origin 可为退出协调中的非 active buffer，不把操作转发给当前碰巧聚焦的其它目标。旧会话和旧输入快照的迟到结果均被隔离。

### 候选式读入（阶段 B）

- 命令、文件和 buffer 读入进入即显示候选，已提交输入变化时更新；输入文字、候选集合、选中项与接受动作独立。候选显示在输入行下方，最多 8 行（窗口空间不足时减少），选中项保持可见，显示位置 / 总数。命令注释为当前正文键位（未自定义时为默认键位）；buffer 注释为修改标记和文件路径；文件注释区分文件 / 目录，不读取文件内容。注释不参与匹配，命中字符高亮。
- 匹配采用 fzf 风格评分式有序子序列：字符顺序不变，允许间隔，按连续 / 边界命中及间隔等评分排序，精确匹配优先，同分按名称稳定排序；不承诺与 fzf 的精确分数或顺序一致。smart-case：无大写输入忽略大小写，有大写则区分。空输入不按匹配分数排序，沿用稳定名称顺序及原有 buffer 默认项；不做历史排序、Orderless、多词 / 正则等搜索语法。
- 文件只列出实际目录的直接子项：目录部分按既有路径规则解析，最后一段名称参与模糊匹配，保留输入的路径前缀，不递归扫描目录树。搜索的大小写策略不改变文件身份或磁盘文件名。
- `C-n` / `C-p`、方向键上下移动选择，不改变输入文字、不循环；在首候选之上可选择输入行。修改输入后优先精确匹配，否则首项；空 buffer 输入优先原有默认项，完整目录输入预选输入行。
- `TAB`（含 `C-i`）填入选中候选而不结束会话；选中输入行时有候选则填入首项。目录填入后显示其子项。`RET` / `C-m` 接受选中项；选中目录则进入而不尝试打开目录；不更改 DEL / M-DEL 的编辑语义。
- `M-RET` 直接接受原始输入，或者先向上选中输入行再 `RET`。命令 / buffer 输入必须为实际存在的精确名称，不因原始输入接受而创建 buffer；空 buffer 输入仍使用原默认值。空 `M-x` 默认选中首个命令，`RET` 执行该命令；原始空命令或无匹配名称提示并保留会话。
- 文件非空且无候选时 `RET` 接受新路径；有候选但要使用原始新名称时用 `M-RET` 或选择输入行。空文件名不能接受；原始目录不能作为文件接受。保存覆盖、外部修改等仍经过既有确认，不被候选 / 原始输入接受绕过。
- 预编辑不参与过滤；组合期间冻结候选展示，编辑器的选择、补全及接受沿用 IME 命令屏障。已提交文字更新后再刷新候选。异步刷新期间保留上一批候选、选中项及其布局，新结果就绪后整体替换，不先清空列表；保留的旧候选只用于显示，补全 / 接受仍等待当前输入的有效结果。异步结果绑定会话与输入快照，旧结果不改新输入或新会话。
- 字体名自由输入、单键查询与完整 yes / no 查询不显示候选，不引入新的默认同意路径。prompt 保护、取消、origin / 焦点恢复和输入会话隔离沿用既有约定。

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
- 保存固定请求时的 committed 快照、格式与保存点，后续编辑不因旧快照完成而变 clean。排队的“保存当前文件”跟随已成功完成的另存目标；实际写入目标、文件身份和保存点必须一致。失败或取消停止该 buffer 后续排队的保存，避免静默回退到旧路径；不清 dirty、不更新文件身份或保存点、不破坏原文件。
- kill dirty buffer、退出与窗口关闭按 modified 确认；取消后保留正文与焦点。组合状态按上一节处理，已有 dirty 不因丢弃预编辑消失。
- 普通覆盖 / 外部修改 / kill dirty buffer / 退出确认走 minibuffer 查询会话：覆盖、退出保存用单键 y / n，kill 修改确认用单键 y / n / s；外部修改及最终退出要求完整 yes / no 后 RET。退出保存不提供 Emacs 的 !、.、q 等附加动作，保留已登记的 F2 简化。RET 不代表默认同意；`C-g` 取消且不执行破坏性动作，取消后恢复 active 正文焦点，原操作目标不变。
- OS 窗口关闭等平台触发且无法可靠进入 minibuffer 的确认可使用窗口级确认框；该路径必须安全默认 Cancel、Escape / `C-g` 取消，只有明确选择后才执行破坏性动作。此平台例外不得扩散为其他确认的默认实现，并须登记。

## 7. 显示设置

本节定义阶段 A 的产品行为，不追求 Emacs 外观或 text-scale-mode 的完整兼容；实现与验收状态只在 [实施计划](plan.md#阶段-a显示设置) 维护。

### 字体与字号

- 正文与 minibuffer 共享应用级字体、字号，已有及新建编辑视图均使用当前设置；mode line、echo area 等外围 UI 暂不跟随，不表示长期必须使用割裂的字体体系。
- 提供设置字体族、增大字号、减小字号和恢复基准字号的命令，先通过 `M-x` 使用，不新增默认键位。每次增减 1 逻辑像素，字号须保持在有效范围内；默认使用随包 Lilex，原样包含 Regular / Bold / Italic / BoldItalic 四变体，在首个视图排版前加载，不依赖系统安装；初始 16 逻辑像素，行高仍为字号的 1.5 倍。中文和 emoji 等缺字继续使用平台回退，不保证其强调变体存在。
- 当前运行字号与基准字号分开，恢复操作只恢复字号、不撤销字体族设置；行高随字号按既有比例变化。不提供 buffer / view 独立缩放或多层样式覆盖。
- 字体族名称去除首尾空白，拒绝空白名称及控制字符；用户可指定其他具体字体族，名称解析与缺字处理仍交给 GPUI / 平台字体回退，不额外解析 `monospace` / `sans-serif` 等通用别名，也不合成斜体；不将设置成功的消息当作字体已安装、实际命中或具有所请求变体的证明。不提供字体枚举或选择器等完整外观配置 UI。
- 交互式设置仅在本次运行有效，不自动写回文件；启动基准值可由阶段 D 的 JS 配置提供。
- minibuffer 高度随行高调整，不因放大字号裁切单行输入。字体变化只改变显示与必要的布局 / 滚动，不修改正文、point / mark、历史或 modified；焦点所在 point 保持可见。
- 不兼容的旧布局只能用于保留正文显示，不能继续提供错误的输入坐标；沿用既有异步版本隔离与 IME 命令屏障。

### 方块光标

- 正文与 minibuffer 使用实心方块，覆盖处的文字仍须清晰可见。普通字符、中文、emoji 和组合字符按实际显示字形簇确定覆盖宽度，不将显示边界变成 point / 字符运动的新边界。
- TAB、行尾和空行使用一个空格宽的方块，不覆盖整段 TAB 空白。
- 预编辑期间按显示投影定位，保留预编辑标识；视觉光标形状不改变平台输入范围、命中或 IME 候选定位的语义。
- 保持现有焦点显示策略，本阶段不增加闪烁或失焦空心光标。

## 阶段 E0：基础主题与命名样式

- **模型**：应用级一个当前主题加用户显式样式覆盖；内置 `light` / `dark`，初始为 `light`，不自动跟随系统。不叠加多个主题、不提供任意样式继承、不兼容 elisp 主题或 Emacs font-lock / face 机制。E0 只定义颜色；后续语法启用见 E1 / E2，当前文字属性及排版边界由下文 E3 扩展，仍不开放逐段字体族 / 字号。
- **设置**：沿用 JS 配置入口声明主题、初始选择和命名样式覆盖，名称及可用颜色属性见 [配置 API](scripting.md#主题与命名样式)。主题缺省属性从同明暗模式的内置主题补齐，最后应用用户显式覆盖；切换重新解析，不保留旧主题的未覆盖属性。非法名称、颜色、属性或重复主题定义拒绝整份配置，不部分应用。
- **切换**：`M-x load-theme` 使用既有候选读入，无新增默认键位；确认有效名称后整体切换，取消或无效名称不改变当前主题。交互切换仅本次运行有效，不写回配置，成功重载以新配置为准；失败保持旧主题、定义和覆盖。沿用现有 IME 与读入会话屏障。
- **显示**：正文、新旧 / 隐藏 view、minibuffer、region、方块光标、选中 / 非选中 mode line、候选 / 命中 / 注释、消息和分隔线使用统一有效配色。光标覆盖文字使用独立前景色，提示词着色不进入文本。颜色变化不改变正文、point / mark、历史、modified、scroll、换行索引或字形布局，只按当前颜色重绘；超长字形行也不等待重做 shaping 或丢失输入坐标。异步布局继续按目标和文本 / 布局身份校验，不因换色丢弃可用几何结果。自定义配色的可读性由用户负责，不自动改写所选颜色。

## 阶段 E1：Rust tree-sitter

- **启用**：文件路径的 `.rs` 后缀自动启用 Rust 语法；成功另存后按新路径重新判断。scratch、minibuffer、消息历史和其他文件保持普通文本，不按内容猜测语言，不提供手动 mode 切换或 JS 语言 / 查询接口。
- **解析**：Rust 核心在后台增量解析 committed 文本，所有 view 共享 buffer 的解析结果。编辑、undo / redo 和平台提交仍走唯一 Edit 入口；结果按 buffer、文本版本及语言身份隔离，过期结果不着色新正文。尚无可用配色、解析不可用或查询失败时暂用普通文本，不阻止编辑或隐藏正文；不完整 / 含语法错误的 Rust 文件仍尽量着色。
- **样式**：关键字、注释、字符串、数字、类型、函数、变量、常量、运算符和标点使用中性语法样式，名称与配置见 [主题 API](scripting.md#主题与命名样式)。这是语法着色而非语义分析，不承诺精确识别名称绑定、宏展开或注入语言。
- **显示**：E1 的仅前景色配置不改正文、point / mark、history、modified、scroll、换行或 shaping；E3 扩展的字体、装饰与背景行为见下文。region 保留语法前景色，方块光标覆盖文字的独立前景色和 prompt 颜色优先。预编辑不送入解析器，其文字使用普通前景色；已提交文字按显示投影着色，不让预编辑位移导致后续文本颜色错位。仅绘制属性的异步高亮到达不失效可用布局或输入坐标；字体属性变化按 E3 准备匹配布局。编辑后等待解析期间，未改动字节按编辑映射保留临时配色，插入 / 替换文字暂用普通前景色；连续编辑及历史操作同样逐次映射，不沿用旧坐标。临时配色不表示当前快照已解析完成；当前结果到达后整体更新，切换语言或解析 / 查询失败时清空临时配色。
- **边界**：本阶段不包含语言缩进、格式化、结构导航、语义高亮、LSP 或完整语言平台。解析 / 高亮就绪另作性能基线，不据此放宽既有输入与绘制预算。

## 阶段 E2：多语言 tree-sitter

本节扩展 E1 的语言选择和嵌入范围；E1 的共享后台解析、快照隔离和失败降级规则继续适用；仅前景色显示边界由下文 E3 扩展，不改写 E1 / E2 冻结验收记录。

- **文件关联**：按下表中大小写敏感的后缀选择语言；成功另存后重新判断，不按正文猜测。JSX 与 JavaScript 共用 grammar，TSX 使用独立 grammar。scratch、minibuffer、消息历史和未关联文件仍为普通文本，不提供手动 mode、语言安装器或 JS 语言 API。

  | 语言 | 后缀 |
  |---|---|
  | Rust | `.rs` |
  | JavaScript / JSX | `.js`、`.mjs`、`.cjs`、`.jsx` |
  | TypeScript | `.ts`、`.mts`、`.cts` |
  | TSX | `.tsx` |
  | Markdown | `.md`、`.markdown` |
  | HTML | `.html`、`.htm` |
  | CSS | `.css` |

- **Markdown**：块级与行内着色包含标题、强调、链接、行内代码及 grammar 默认 GFM 表格等结构；文字强调按下文 E3 使用独立角色，不改变字号、不提供预览。围栏语言接受上述后缀名及 `rust`、`javascript`、`typescript`、`markdown` 等完整语言名，别名忽略 ASCII 大小写。已注册语言的围栏内容使用子语言高亮；未知 / 缺失语言保持普通代码文本，不递归展开围栏内的进一步围栏注入。嵌入 Markdown 本身仍有块级 / 行内高亮。
- **HTML**：沿用 grammar 的 `script` / `style` 原始文本范围，分别按 JavaScript / CSS 着色，包括 Markdown HTML 围栏中的这两类范围。不依据 `type` 属性解释 MIME、TypeScript 或模板方言，不解析标签属性内的事件处理代码或 CSS。
- **注入边界**：只执行 Markdown 行内 / 围栏与 HTML script / style 规则；不自动启用 Markdown 原生 HTML、元数据、LaTeX，或 JS tagged template、正则、JSDoc 等其他 injections。子解析使用 committed 文本的受限范围，排除 Markdown 引用 / 列表的续行标记；与主解析一并取消、按文本及语言身份提交。跨语言另存不复用旧语言树或暴露迟到高亮。
- **覆盖与降级**：同层较内侧的 capture 优先，相同范围由后面的查询规则优先；子层仅在自身内容范围内覆盖宿主已有颜色，未着色位置保留宿主样式。`@none` 明确恢复普通前景色。标签及属性等适配到中性语法样式，标题 / 强调按 E3 使用独立角色；不引入完整 face 或语义绑定分析。每个快照最多 16,384 棵树，子层深度最多 4；超过边界的剩余子内容保留宿主颜色或普通文本，正文和编辑不受影响。解析或查询失败仍按 E1 暂用普通文本，不阻止编辑。

## 阶段 E3：文本样式与高亮合成

- **属性**：沿用命名样式及按属性覆盖，正文基准、prompt、语法及 Markdown 角色可设置前景 / 背景、普通 / 加粗、普通 / 斜体、下划线及删除线；其他外围角色继续使用既有颜色属性，不允许选区 / 光标改变字体。具体名称、类型及限制见 [主题配置](scripting.md#主题与命名样式)。未指定不覆盖，普通 / false 可显式取消强调；不提供逐段字体族 / 字号、任意 face 继承或波浪线。
- **Markdown**：标题、强强调和强调分别使用 `markup.heading`、`markup.strong`、`markup.emphasis`，内置分别加粗、加粗、斜体。嵌套强调按从外到内的显式属性合成，可形成粗斜体；行内内容继承标题 / 强调，围栏子语言不继承宿主文字强调。
- **范围与行背景**：Rust 内部可向共享 buffer 安装绑定当前快照的文字范围样式及逻辑行背景，不依赖语法解析、不修改正文、point / mark、history 或 modified。两组范围各自须有序、不重叠且合法；安装失败保持原样，过期 / 跨 buffer 安装拒绝，文本编辑后清除旧注记，不猜测新范围。整行背景从正文视图左缘铺到右缘，覆盖软换行的各视觉行、行尾空白、空行及被指定的 EOF 空行；预编辑新增视觉行按已提交插入点所属逻辑行映射。尚无 JS 范围装饰 API、diff 专用角色或通用 overlay / marker 框架。
- **合成优先级**：背景由低到高为正文底色、逻辑行背景、文字自身背景、外部范围背景、region、方块光标；region 保留语法前景色及文字字体 / 装饰，光标只覆盖文字颜色及其方块背景。外部范围只覆盖自己显式指定的属性；prompt 的命名文字样式优先于正文范围样式，光标前景色最高。预编辑不解析，使用基础输入文字样式，不套用提交文字的范围属性，并保留专用预编辑下划线。
- **排版与坐标**：颜色及普通装饰只重绘，不重新解析或失效布局；字重 / 斜体参与测量、软换行与 shaping，不能假设等宽字体的所有变体宽度相同。全文换行按有效字体 / 字重 / 斜体缓存的字素宽度计算断点与跨软行 TAB phase，不逐候选行做上下文 shaping；任意字体、连字及复杂文字的断点不保证等同全文逐行精确排版，可见行的真实宽度也可能超出换行宽度。实际可见绘制、命中、光标、选区及 IME 坐标仍使用对应字体样式的真实 glyph layout，不以字素宽度之和代替输入几何。字体范围边界若落在字素内部，以字素起始位置决定该字素的字体，不切开组合字素或 emoji 序列。布局及后台结果绑定目标、显示文本、字体 / 尺寸和有效字体样式身份；新字体不套用旧输入坐标。同文本的旧布局可暂供绘制，但不能冒充当前输入几何。字体样式变化尽量保持视口顶部文字锚点，不修改 point；普通文字编辑仍沿用既有 caret reveal 行为。
- **异步与资源**：语法结果仍是主题无关的语义，改主题不重新解析。待解析期间按编辑映射保留未改动字节的临时样式，新增 / 替换字节暂用基础样式；这一映射不表示当前版本已解析就绪。正文和带样式排版保持前台有界预览、后台完成与迟到结果隔离；大语法快照的最后持有者释放也转交后台。不以字体安装成功或 headless 回归证明真实字形 / fallback、平台 IME 或屏幕呈现正确。
- **性能边界**：即时响应、可见几何及全文索引就绪分别观察，后台 ready 不等同于用户等待。既有指标及预算仍见 [性能预算](performance.md)，本阶段不自动放宽；当前语法结果与匹配布局全部就绪继续作为独立基线，不把它套成既有纯布局 ready。

## 阶段 C：buffer 与 view

本节是阶段 C 新增并批准的有效行为；不改写 v1 历史边界或既有冻结验收记录。本文其余编辑语义继续适用；冲突处以本节的多 view 归属规则为准。

- **共享 document / buffer 状态**：同一 buffer 的文本、history、modified、文件身份与保存状态、mark / active 在所有 view 间共享。编辑及 undo / redo 由统一历史管理；undo / redo 将发起操作的 view 的 point 按既有历史规则恢复，其他 view 的 point 按编辑 marker 规则调整。
- **view-local 状态**：point、goal column、scroll、布局及输入会话属于 view。编辑导致其他 view 的 point 依 marker-like 规则调整，但不因此自动滚动。region 高亮只显示在当前选中 view。
- **split 与焦点**：`C-x 2` 在下方 split，`C-x 3` 在右侧 split；新 view 显示同一 buffer，并复制原 view 的 point 与 scroll，焦点仍留在原 view。允许嵌套 split；空间不足时报告错误。不支持拖动调整大小或多个 OS frame。
- **window 辨识**：左右并排的分屏之间绘制细竖分隔线；每个 view 的 mode line 按是否选中使用不同的主题配色。minibuffer 会话中保留会话前正文 view 的选中标识，不把 minibuffer 获得输入焦点视为另选正文 view。
- **view 命令**：`C-x o` 切换到下一个 view；`C-x 0` 关闭当前 view；`C-x 1` 保留当前 view 并关闭其余 view。不能关闭最后一个 view。关闭 view 不杀 buffer、不询问保存。`C-x b` 与 find-file 只影响选中 view。
- **buffer 生命周期**：kill buffer 沿用既有 dirty 确认；确认后关闭该 buffer 的所有 view，并在原有 split 布局中将它们替换为一个存活 buffer，若无可用 buffer 则替换为 scratch。关闭 view 本身不触发 dirty 确认。
- **输入与会话**：切换 view 会打断连续输入历史合并与连续 kill 链，并清除旧 view 的 pending 前缀 / 输入会话。既有 IME 命令屏障、目标绑定与迟到回调隔离继续适用；指针失焦沿用既有丢弃 marked 预编辑策略。minibuffer 会话期间禁止切换或关闭正文 view；结束后恢复会话前选中的正文 view，操作仍使用原目标（退出确认的目标可以是非 active buffer）。输入回调绑定 view 与输入会话，异步结果按各自请求的目标 / 快照校验，不随焦点变化改投目标。

## 阶段 D1：消息显示与历史

- **显示归属**：echo 是窗口级临时状态，不随切回旧 buffer / view 恢复旧消息。没有读入会话时展示 pending 前缀、命令消息及错误；正常后续命令 / 输入清除旧提示。
- **底部布局**：echo 与 minibuffer 共用底部区域，不在读入面板之外另占空 echo 行。普通消息支持换行与自动折行，显示高度最多为窗口高度的四分之一，至少保留一行；超出内容裁切，不覆盖 mode line，完整消息从历史回看。尺寸随当前内容及时收缩，不精确复刻 Emacs 的 `grow-only` 高度保持策略（F2 布局差异）。
- **读入期间**：普通消息临时以 ` […]` 附在输入末尾，下一次正常输入 / 命令时清除；补全、无匹配及查询答复提示另在两秒后清除，以新输入与超时中先到者为准。提示只是显示注记，不进入 committed 文本、point / mark、undo、候选查询或 IME 输入快照，不抢走焦点；新消息与结束会话使旧提示计时失效。附注也受显示高度及裁切约束。
- **消息历史**：命令消息和错误写入只读 `*Messages*`；pending 前缀与上述读入提示不写入。最多保留最近 1000 个逻辑行，超出从头裁去；完整保留限额内的长 / 多行文本，相邻相同的单行消息折叠为 `[N times]`，多行消息不整体折叠。历史更新不产生用户 undo 或文件 dirty。
- **回看**：`C-h e` / `M-x view-echo-area-messages` 显示历史并定位末尾，优先复用已显示该 buffer 的 view 或其它未选中 pane，否则尝试分屏，不主动选择另一个正文 pane。空间不足以分屏时在当前 pane 显示，这是现有窗口系统下的 F2 布局回退；滚动、选择、复制与切回正文复用既有命令，不新增独立弹窗或完整 Emacs special-mode 命令集。
- **buffer 生命周期**：首次消息或显式回看后，历史可通过普通 buffer 读入选择 / 关闭；内部创建不改变正文的 MRU 顺序。关闭后下一条消息 / 回看创建新历史，不恢复已关闭内容。成功另存历史后，后续消息写入新的日志，不继续改写已另存的 buffer；若保存请求后、完成前已有新增消息，已另存 buffer 保留这部分未落盘内容并显示 modified，可再次保存，不把异步快照之外的内容标作 clean。

## 阶段 D：JS 配置与文本变换

本节定义已批准的最小脚本接入范围，不兼容 elisp，不扩大为任意插件系统。使用入口和宿主 API 见 [JS 配置](scripting.md)。

- **启动**：单一 `init.js` 声明字体基准、正文键位及脚本命令。配置缺失等同空配置；执行或校验失败回退默认值并报告错误，不部分应用。首次配置完成或失败后才开放可编辑正文，不先使用默认键位 / 字体再异步切换；等待配置不阻塞 UI 线程。
- **显式重载**：`M-x reload-configuration` 在旧配置继续生效时准备新配置；执行和 Rust 校验全部成功后，整体替换配置所属的设置、键位、脚本命令及运行时。删除声明会恢复默认设置 / 键位并移除旧脚本命令；失败保留旧配置。交互式字体设置不自动写回文件，成功重载以配置基准为准。成功重载清空 pending 前缀并使旧脚本结果失效，不承诺任意模块 / 订阅 / 后台任务热重载。既有 IME 屏障继续适用；准备期间若开始组合输入，安装时保留旧配置并提示完成组合后重载。
- **统一命令**：脚本在配置阶段注册命名的文本变换函数；配置结束后不能动态增删注册。原生和脚本命令共用名称解析、`M-x` 候选和键位绑定来源，原生命令保留不经过 JS 的直接执行路径。一个命令可绑定多个键；脚本不能覆盖原生命令身份，重复脚本命令名也被拒绝。
- **正文键位**：自定义键位不改变 minibuffer / 确认会话规则。允许显式替换默认叶子绑定；取消绑定阻止向默认层回退，但不删除命令及其其他绑定。重复的自定义键序列（包括 Meta / ESC 别名）被拒绝。命令与前缀冲突拒绝整份配置，不隐式删除前缀子树；可显式取消旧命令后为该键定义新的前缀子键，最终结果不依赖声明顺序。
- **文本变换**：只读取发起 view 的非空活动 region，JS 接收字符串并同步返回字符串；没有 region 时报错，不自动处理整个 buffer。JS 在专用 worker 执行，等待结果时原生编辑仍可使用；不支持 Promise / 异步脚本命令。成功结果通过唯一 Edit / 历史入口作为一次独立编辑应用，point 位于替换末尾、mark 按普通编辑规则调整并停用；共享 view、modified 和 undo / redo 沿用既有语义。
- **结果隔离**：请求绑定原 view、buffer 文本版本、输入会话、region / point 及配置代次；文本、区域或输入会话变化，目标关闭 / 隐藏，或成功重载后，旧结果不得提交。提交时重新检查只读、IME 与 pending 前缀约束；不按返回时的焦点重新解释目标，不覆盖后续编辑。
- **能力与资源**：不开放文件、网络、进程、定时器、脚本后台任务或任意模块加载 API；配置与命令有执行、运行时内存 / 栈、源码 / 文本大小和请求数量上限。异常、超限或非法返回值不修改正文；异步 job 不被执行。运行时中断是协作式的，不等于可强杀的独立进程，也不是处理恶意代码的安全沙箱。具体实现限额见[配置说明](scripting.md#资源边界)，不替代或放宽原生性能预算。

## 阶段 F1：mode 与只读 Magit diff

### Mode 与键位

- 主要交互 mode 属于 buffer，多 view 共享；它独立于语言解析 / 高亮身份。首批身份为 `fundamental`、`magit-diff`；不提供 JS mode 注册、hook 或 minor-mode 体系，不承诺这些身份是永久封闭集合。
- 正文查找按 mode 局部层、正文通用层排序，minibuffer / 查询会话不使用正文局部层。mode 默认键位与对应用户声明在配置阶段合成，热路径不执行 JS。Scope 仍表示执行所有者，不表示 mode。
- 局部 `null` 在该 mode 内遮蔽通用绑定，不影响其他 mode；删除声明后恢复默认。局部前缀的缺失子键可回落到通用层；局部命令或禁用节点遮蔽低层整棵前缀。局部前缀可遮蔽通用叶子，但同一层把默认叶子改为前缀仍须显式取消旧叶子；取消仍有局部子绑定的前缀报错。重复检查按层进行，任一错误拒绝整份配置。
- 按键与 M-x 都校验目标 buffer 的命令适用上下文；M-x 不列出当前 mode 不适用的命令，手输 / 通用绑定也不能绕过检查。键位注记按当前层叠结果显示。
- 成功配置重载保留 diff 的比较身份、内容、折叠及 view 位置；失败保留旧配置。原有输入屏障、pending 清理及整体安装语义不变。

### 独立 diff 查看

- `M-x magit-diff-unstaged` 查看 index → worktree，`M-x magit-diff-staged` 查看 HEAD → index；未有首个 commit 时仍可查看 staged 新增文件。仓库从发起文件的目录发现；在 diff 中切换比较时沿用该仓库。比较针对磁盘 / index，不自动保存编辑 buffer，也不把未保存文本当作 worktree 内容。
- 显示 diffstat、文件与 hunk heading 和 unified 正文；普通文件 / hunk 默认展开，删除文件默认折叠。binary、symlink / 文件类型及 submodule 变更只提供摘要；未合并 / combined 等不支持结构明确报错，不伪造逐行 hunk。
- `TAB` / `C-i` 切换当前 section 正文，保留 heading；root 不能隐藏。父节点折叠不改写子节点自身状态；隐藏正文中的各 view point 移至折叠 heading。正文仍保留在只读 buffer 中，普通 region / 复制沿用文本语义，不提供 Magit 结构化选择语义。
- `n` 到下一个可见 section heading；`p` 在正文先回当前 section 起点，在起点再到前一个可见 heading；`^` 到父节点。`j` 在 diffstat 文件项与对应文件间跳转，必要时打开目标祖先。SPC / DEL / S-SPC 及原通用翻页键按可见行翻页，普通文本移动跳过隐藏正文。
- 当前 section 提供独立于 region 的背景反馈；折叠 heading 带指示。长行不软换行，M-右 / M-左（`scroll-left` / `scroll-right`）水平查看，point 移动到屏幕外列时跟随。普通编辑 buffer 的软换行不变。
- `g` 重新读取相同比较。按 section 类型 / 路径 / 父链及 hunk heading 身份尽量保留折叠、各 view point 与顶部 section 相对行列；节点消失时回退到最近仍存在的祖先 heading，不能保证 hunk 内容变化后仍匹配。只读刷新没有文本 undo，也不标记 modified。
- `RET` / `C-m` 在删除行访问旧侧，在新增 / 上下文行访问新侧并定位；heading 使用对应侧起始行。HEAD / index 使用本次 diff 的完整 blob 身份打开只读快照，不重解释为当前 index 或 worktree；unstaged 新侧复用普通文件 buffer，保留其未保存文本，行列超界时夹紧。不提供 C-RET 的跨版本行号映射。
- Git 在后台以参数数组执行，关闭外部 diff / textconv，不运行 Git 写命令。每窗口最多一个 Git 读取，无请求队列；单次子进程 15 秒，stdout 16 MiB、stderr 64 KiB，diff 最多 4096 个文件 / 100000 行 patch，超限明确失败。禁止按需自动抓取缺失 blob；固定 worktree / git-dir，刷新不能意外回落到父仓库。刷新失败保留旧内容；结果校验目标及请求身份，不按完成时焦点重新定向。打开 / 版本访问期间焦点目标变化或进入读入会话时不抢回界面。

首版不包含 status、transient、Git 写操作、任意 revision / range、patch 导入、untracked 浏览、结构化 / 部分 hunk 选择、词级差异、hunk 语法、递归折叠全套、兄弟导航、搜索自动展开、diff 参数修改或窗口恢复式 q；后续范围以计划为准。
