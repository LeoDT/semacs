# 实施计划

发布边界：S1–S5 是正式实现的前置门禁，M0–M3 构成 v1；M4 / M5 是后续阶段，不阻塞 v1。
当前 S1–S5 的机制验证已落在 `spikes/`（逐项证据见 `spikes/EVIDENCE.md`）：
interceptor、前缀 / C-g、自建 undo、平台输入与 IME 观测、mark marker 语义、鼠标导入、多 buffer 隔离、
真实键位分类、IME 命令屏障、窗口关闭 / 焦点切走、真实 IME 提交后的 mark、redo point 均已实测并对齐。
可选剩余：真实硬件键盘 / WM 保留键确认；WM 强退与进程崩溃未测。

## 1. 先做 spike（不写正式代码）

在正式铺 M0 之前，用最小验证工程检查下面 5 件事。依赖来自 crates.io，保存 `Cargo.lock`；
本地源码只作参考。临时工程放 `.pi/tmp/`，通过后的测试在实现阶段迁移为正式回归用例。

| ID | 验证内容 | 通过标准 |
|---|---|---|
| S1 | interceptor 与平台输入边界 | 被消费的键既不触发底座 action 也不插字符；普通 / Shift 字符正常；普通 Meta 不误插入；IME 可正常预编辑与提交；只读目标不会被直接输入 handler 绕过；pending 前缀期间的直接文本回调遵守前缀解析，不绕过为自插入 |
| S2 | 前缀、undefined 与取消 | `C-x` pending 正确且无超时；`C-g` 取消 pending / minibuffer 并停用 region、保留 mark；用双方当前 keymap 均未绑定的序列断言 undefined 回显且不插字符；最小 minibuffer 经公共 EditTarget 验证 motion / 删除 / kill / yank、prompt 保护与焦点路由 |
| S3 | 真实命令入口的 undo 与完成顺序 | 经 interceptor → 自有命令 → EditTarget（正文为 Buffer）跑保真度矩阵；验证自有删除、输入、换行、kill / yank 的分组；验证连按 undo、undo 后立即输入 / 切换 buffer / 进入 minibuffer；正文与 minibuffer 历史独立，caret、目标、错误回显与保存点语义正确 |
| S4 | 外部变更观测与 point / mark / region | 验证能取得足够的替换范围和内容；覆盖自有编辑、历史回放、正反向拖选、IME 多次更新 / 提交 / 取消及指定范围替换；mark 位置与激活状态独立，同步不破坏拖选 / IME、不回环；通过下述组合输入交错与跨目标状态矩阵 |
| S5 | Wayland 输入分类与键名 | 记录 `C-?`、AltGr、死键、`RET` / `C-m`、`TAB` / `C-i`、Shift 字符的原始事件与文本回调；普通命令修饰键、文本输入和系统保留键分类符合预期 |

S1–S4 必测的跨边界场景（先固化 Emacs / 平台预期，再验证候选实现）：

- 预编辑中触发编辑命令、`C-g`、保存、前缀启动、切换 buffer、进入 / 退出 minibuffer、kill / close；
  验证提交 / 取消的选择、输入完成屏障、命令执行顺序和目标身份。IME 自己消费的候选操作与编辑器命令分开记录。
- pending 前缀期间直接文本提交、焦点切换后的迟到 IME 回调、minibuffer 取消后重开；
  不得绕过前缀插入或污染新目标，不把焦点变化当作原输入完成证明。
- 在最小文件保存用例中验证：预编辑不落盘；提交 / 取消完成后才保存已提交文本；
  同时断言文本、point / mark、dirty、undo 与磁盘内容，不能只看最终界面。
- A 设置 point / mark → 切到 B 并编辑 → 返回 A；包含 active / inactive mark 和不同滚动位置。
  每个 buffer 的状态、历史与高亮不串用，激活状态按基准的切换命令语义断言；关闭后迟到事件不修改其它目标。
- 正文与 minibuffer 使用同一组编辑命令，验证 `C-a` / `C-e`、删除、`C-k` / `C-y`、mark、undo；
  双向 kill / yank 共享全局 kill ring，undo 只改变当前目标，prompt 不可编辑。

交付与失败处理：

- 已完成的证据：`spikes/reference/RESULTS.md`（Emacs 31.1 真实 command loop）、
  `spikes/EVIDENCE.md`（实现事实与未覆盖项）、`spikes/lab/cases/*.expected`（headless 断言）。
  `mark-*` 与 `s3-mark-*` 覆盖 marker 跟踪；`s4-ime-close` / `s4-ime-focus` 覆盖关闭与失焦；
  `real-cases/s4-ime-mark` 覆盖真实 fcitx5 提交后的 mark 跟随；`s4-mark-ime` 保留为适配器驱动的合成回归。
- 先按 [保真度基准](emacs-fidelity.md#5-对照基准与-undo-验收矩阵) 固定参照脚本、模式、选项与初始状态，
  再记录双方结果。`C-x a` 在参照 Emacs 中是前缀，不能用作 undefined 基准。
- 导出并核对全部 v1 默认键位的 Emacs 命令名及 semacs 注册表；保存参照模式、选项、绑定结果与差异。
  当前已核对 `C-a` / `C-e` / `C-x C-c`，不以这三项代替全表检查；语义正确也不能掩盖 F1 命令名错误。
- S3 / S4 输出实际命令链路、公共 EditTarget 的适配机制、状态归属、编辑范围来源与完成时机；
  固定 IME 与命令交错时的提交 / 取消策略。仅证明 action 能执行、焦点正确或最终文本正确不算通过。
- 每项记录测试命令、环境、依赖版本、实际结果与未解决问题；Wayland 的 IME 验证单独留证。
- 任何一项失败或关键行为无法观测，停止进入 M0，提交最小复现与 API 缺口。
  接受新增差异、补上游 API、依赖 patch 或更换底座都须另行确认，不能在 spike 中默认拍板（D14）。

## 2. 里程碑

里程碑按"可验收的行为"划分，不按时间估算。每个里程碑结束都要能运行、能测试。

### M0 骨架

内容：

- cargo 工程、依赖、应用启动、窗口与主题；
- 命令行文件参数或 scratch buffer；
- `EditorState` 渲染、mode line、echo area；
- 未保存内容的窗口关闭确认（尚未支持保存时，取消或明确确认丢弃，不静默丢失）；
- 性能测试样本与基线：记录硬件、构建模式、文件字节数 / 行数 / 长行与 Unicode 分布、打开耗时和滚动表现。

验收：`cargo run -- <file>` 能显示文件内容，可以输入文字，中文输入正常；
关闭取消后内容保留；可复现性能基线已记录，明显阻塞或无法打开须先定位。

### M1 键位引擎 + 基础编辑

内容：

- `keys`：`parse_key_sequence`、`Keymap`、前缀匹配、分层覆盖、命令 / 前缀冲突检测、undefined / self-insert 判定；
- interceptor 接入、命令注册表、公共 EditTarget / EditingState、输入目标路由与已通过 spike 的输入完成屏障 / caret 同步协议；
- 基础命令：字符 / 行 / 词 / buffer 首尾移动、翻页，换行 / 基础缩进，删除，`C-k`，`C-w` / `M-w` / `C-y`，
  `C-SPC`，`C-g`，undo / redo；
- AppShell 全局 kill ring 与连续命令链；连续 kill 的追加规则，以及输入 / 其它命令打断追加的规则；
- echo area 显示 pending 前缀与错误。

验收：

- 纯键盘完成基本编辑；
- 键位引擎单测通过；
- 交互测试覆盖 motion、kill / yank、`C-g`、undo / redo，并用测试命令验证多键前缀派发；
  `C-x C-s` 的真实保存测试随 M2 提交，不以空实现充当通过；
- 未定义键不插入且回显，Meta / Shift / AltGr 分类与 S5 结果一致；
- mark 是 marker（编辑 / undo / redo / IME 提交后跟随），与激活状态独立；输入提交后的连续 kill 中断均有断言；
  redo point 与 Emacs 已对齐；
- 回归 S3 / S4 的真实链路用例，记录按键到显示更新的延迟基线与测量方式。

### M2 minibuffer、M-x、文件与 buffer 管理

内容：

- `M-x` / `execute-extended-command`，从唯一命令注册表读取名称并执行；
- `find-file` / `save-buffer` / `write-file` / `switch-to-buffer` / `kill-buffer`；
- dirty、保存点、kill dirty buffer、`C-x C-c` 与窗口关闭的确认；
- minibuffer 的读取与命令名 / 路径 / buffer 名基础补全，确认 / 取消后的焦点恢复；
- buffer 列表与切换，保存失败处理与覆盖已有文件的确认。

验收：

- 不碰鼠标完成“打开 → 编辑 → M-x 执行命令 → 保存 → 切换 → 退出”；
- minibuffer 的 motion / 删除 / mark / kill / yank / undo 走公共编辑接口；正文与 minibuffer 共享 kill ring、历史独立；
  prompt 不可被编辑，补全不绕过目标编辑入口；`RET` / `TAB` / `C-g` 正确，退出后无残留 pending；
- A → B → A 保留各自 point / mark 与滚动位置，激活状态符合基准，kill-buffer 正确释放状态，不串用历史或高亮；
- 回归预编辑中保存、切换、进入 / 退出 minibuffer 与关闭：先收敛旧输入再执行命令，不保存临时文本，不接受污染新目标的迟到回调；
- 保存失败不清 dirty、不更新文件身份或保存点、不破坏原文件，错误回显可见；
- 覆盖已有文件、kill dirty buffer、退出和窗口关闭支持确认 / 取消，取消后内容与焦点保留；
- 保存 → 编辑 → undo，以及保存 → undo / redo 的 dirty 与磁盘保存内容一致；
- 打开失败、无效 UTF-8 与不支持的换行输入不得静默丢弃或转换内容；行为在文件层明确并测试。

### M3 v1 保真度验收与打磨

内容：

- 对 v1 已包含的键位 / 命令名、mark / region、kill ring、undo、minibuffer、跨 buffer 状态与组合输入交错 / 退出流程逐项补齐回归；
- mode line 的 buffer 名 / dirty / 行列与回显细节；
- 已登记差异的回归测试与用户可见说明；
- 复测 M0 / M1 性能基线，定位回归，记录尚存限制。

验收：v1 范围内 F1 逐条通过；范围内 F2 差异有记录，F3 与实际行为相符；
S1–S5 的测试均已迁移或保留可复现运行方式。未批准的新差异不得作为发布豁免。
`M-y`、`C-u`、isearch、split 不属于本里程碑。

### M4 后续能力评估（不属于 v1）

内容：

- 按实际需求独立决定 isearch、`M-y`、`C-u` / 数字参数及更多重复语义的优先级；
- 评估不同 buffer split 与 `C-x 1/2/3/o`；先明确 `C-x 2/3` 默认显示同 buffer 与现有底座限制的冲突，
  不承诺标准 split 语义已可实现；
- 同 buffer 多视图是否需要自研 view，另行决策，不默认启动方案 C；
- 若引入 split，扩展现有性能与焦点路由测试，而非首次测量性能。

验收：只针对获批的后续能力定义；若实现 split，焦点 / 命令路由与限制提示均正确，键位差异已确认。

### M5 diff 与 magit（后续阶段）

内容：

- diff 查看器（gpui-kit `component::diff`）；
- magit 风格只读 buffer：section 折叠、`TAB`、`RET`、transient keymap、刷新；
- 只读 buffer 的 keymap 机制在 M1 的键位引擎中已具备，不另起框架。

## 3. 测试策略

- **纯单测**：键名解析、序列匹配、undefined 判定、文本变换、kill ring、文件名处理。
- **交互测试**：gpui-kit 的 `test-support` + `VisualTestContext`，用
  `cx.simulate_keystrokes("ctrl-x ctrl-s")` 驱动真实分发链，断言文本 / point / mark / echo。
- **对照测试**：按 [固定基准与矩阵](emacs-fidelity.md#5-对照基准与-undo-验收矩阵) 验证，
  同时断言文本、point、mark 位置 / 激活状态、dirty、echo 和输入目标，不只看底座结果。
- **文件安全测试**：成功 / 失败保存、覆盖确认、保存点与 undo、kill / close / quit 的确认与取消。
- **性能基线**：M0 记录打开 / 滚动，M1 增加按键延迟，M3 复测；样本、硬件、构建模式和测量方式可复现。
  量化预算在首轮测量后确认，未测量不写“性能通过”。
- **手工验收**：Wayland 下的 IME 更新 / 提交 / 取消、AltGr / 死键、字体渲染；
  系统剪贴板的同步策略确认后才验收相应集成，X11 保持启动与基础输入冒烟测试。

## 4. 任务跟踪

spike 阶段在 `.pi/tasks/spike-tasks.md` 维护剩余项（可选真实硬件确认、文档）；
实现阶段按里程碑建立新的跟踪文件，例如：

```
- [ ] M0 骨架与性能基线
- [ ] M1 键位引擎 / 基础编辑 / marker 回归
...
```

## 5. Definition of Done（每个里程碑）

- `cargo test` 全绿；
- 新增行为有对应测试；
- 文档中的“待决 / 待验证”项有证据更新；影响本里程碑的阻断项已解决，无隐式新增差异或依赖 patch；
- 在 Wayland 上手工验证过关键交互；
- 没有把 EditorState / InputState 泄漏到命令层；正文与 minibuffer 不复制编辑命令语义；
- 对照用例记录完整环境与真实命令路径，相关性能基线未出现未解释的回归。
