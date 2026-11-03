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
- 提供设置字体族、增大字号、减小字号和恢复基准字号的命令，先通过 `M-x` 使用，不新增默认键位。每次增减 1 逻辑像素，字号须保持在有效范围内；初始字体、字号与行高比例沿用既有默认值。
- 当前运行字号与基准字号分开，恢复操作只恢复字号、不撤销字体族设置；行高随字号按既有比例变化。不提供 buffer / view 独立缩放或多层样式覆盖。
- 字体族名称去除首尾空白，拒绝空白名称及控制字符；名称解析与缺字处理沿用平台字体回退，不将设置成功的消息当作字体已安装或实际命中的证明。不提供字体枚举或选择器等完整外观配置 UI。
- 交互式设置仅在本次运行有效，不自动写回文件；启动基准值可由阶段 D 的 JS 配置提供。
- minibuffer 高度随行高调整，不因放大字号裁切单行输入。字体变化只改变显示与必要的布局 / 滚动，不修改正文、point / mark、历史或 modified；焦点所在 point 保持可见。
- 不兼容的旧布局只能用于保留正文显示，不能继续提供错误的输入坐标；沿用既有异步版本隔离与 IME 命令屏障。

### 方块光标

- 正文与 minibuffer 使用实心方块，覆盖处的文字仍须清晰可见。普通字符、中文、emoji 和组合字符按实际显示字形簇确定覆盖宽度，不将显示边界变成 point / 字符运动的新边界。
- TAB、行尾和空行使用一个空格宽的方块，不覆盖整段 TAB 空白。
- 预编辑期间按显示投影定位，保留预编辑标识；视觉光标形状不改变平台输入范围、命中或 IME 候选定位的语义。
- 保持现有焦点显示策略，本阶段不增加闪烁或失焦空心光标。

## 阶段 E0：基础主题与命名样式

- **模型**：应用级一个当前主题加用户显式样式覆盖；内置 `light` / `dark`，初始为 `light`，不自动跟随系统。不叠加多个主题、不提供任意样式继承、不兼容 elisp 主题或 Emacs font-lock / face 机制。主题只指定颜色，不改变字体、字号和字重；tree-sitter 与语法着色留给 E1。
- **设置**：沿用 JS 配置入口声明主题、初始选择和命名样式覆盖，名称及可用颜色属性见 [配置 API](scripting.md#主题与命名样式)。主题缺省属性从同明暗模式的内置主题补齐，最后应用用户显式覆盖；切换重新解析，不保留旧主题的未覆盖属性。非法名称、颜色、属性或重复主题定义拒绝整份配置，不部分应用。
- **切换**：`M-x load-theme` 使用既有候选读入，无新增默认键位；确认有效名称后整体切换，取消或无效名称不改变当前主题。交互切换仅本次运行有效，不写回配置，成功重载以新配置为准；失败保持旧主题、定义和覆盖。沿用现有 IME 与读入会话屏障。
- **显示**：正文、新旧 / 隐藏 view、minibuffer、region、方块光标、选中 / 非选中 mode line、候选 / 命中 / 注释、消息和分隔线使用统一有效配色。光标覆盖文字使用独立前景色，提示词着色不进入文本。颜色变化不改变正文、point / mark、历史、modified、scroll、换行索引或字形布局，只按当前颜色重绘；超长字形行也不等待重做 shaping 或丢失输入坐标。异步布局继续按目标和文本 / 布局身份校验，不因换色丢弃可用几何结果。自定义配色的可读性由用户负责，不自动改写所选颜色。

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
