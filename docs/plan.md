# 实施计划

当前代码提供应用外壳与文件层，尚无正文显示或编辑。按 R0 → R1 → M1 → M2 → M3 推进 v1；
M4 / M5 为后续阶段。里程碑按可验收行为划分，不按工期估算，未实现能力不记为通过。

## R0 文档、组件选择与应用外壳

交付：

- 明确 GPUI、gpui-kit、自有文本 / 视图及 Emacs 命令的职责。
- 记录 Zed Rope / SumTree 的首批提取范围、参考版本及许可门禁；本阶段不导入代码。
- 保持可编译应用外壳，提供文件参数 / scratch、格式信息、mode line / echo area 与编辑区占位。
- 保留独立 Emacs 对照基准、文件编解码与确定性性能样本。

验收：文档链接、构建、格式、静态检查和现有纯单测通过；当前能力与文档一致。
本阶段不提供正文显示、编辑、保存或 IME 编辑，也不以可编译外壳代替编辑器验收。

## R1 最小正式编辑切片

在正式模块中贯通：文本存储 → 自有 Buffer → 显示映射 / view → GPUI 排版、绘制与平台输入。
详细组件与验收清单见 [编辑器实现计划](editor-plan.md)。

交付：

- 完成首批源码导入、归属及许可证整理，保留核心算法和相关测试。
- 文件 / scratch 的正文显示，最小光标、文本输入与替换；命中测试、滚动和软换行。
- 分离已提交正文与临时预编辑；唯一 Edit 入口、文本 / 布局版本、输入目标 / 会话身份。
- IME 预编辑、提交、取消、几何查询和平台坐标刷新；失焦 / 关闭按 D19 处理。
- 输入产生 dirty，已有提交内容的关闭确认正确；预编辑更新 / 取消不置 dirty。
  完整历史与保存点算法在 M1，不用临时内容比较替代 D16。
- 大文件打开、更新、重排、滚动、空帧及输入延迟测量；后台任务过期结果保护。

验收：

- 自动化验证中文、emoji、组合字符、tab、空行、行尾、EOF、换行边界、滚动和非零文字原点。
- 同长度输入 / 预编辑更新也使相关缓存失效；新偏移不查询不兼容的旧布局，布局完成后刷新候选坐标。
- 开启软换行测量 8 MiB 混合、短 ASCII、短 Unicode、长 ASCII；单条超长行独立测量。
  分别记录首帧与最终精确布局就绪时间，不截断正文、不自动关闭换行通过门禁。
- 明确前台工作的边界及等待上限；出现明显同步全文阻塞或无法解释的回退时不进入 M1。
  经首轮测量提出量化预算并确认，不能把等待上限当成全文处理时间。
- headless 与真实平台验收分别留证；真实 Wayland / fcitx5 验证须先获得同意。
  若真实验收尚未完成，保留未完成状态，不宣布 R1 全部通过。
- 本阶段建立可测试的最小输入驱动；完整 Emacs keymap、历史分组与 minibuffer 会话在 M1 / M2 实现。

## M1 键位、编辑语义与历史

交付：

- 自有 keymap / 前缀解析、分层覆盖、命令与前缀冲突检测、undefined / self-insert 分类。
- 命令注册表和统一目标路由；正文与 minibuffer 共享最小编辑语义，不提前完成全部读取 / 补全会话。
- 字符 / 词 / 行 / buffer 首尾移动、翻页、换行 / 基础缩进、删除、kill / yank、mark / region、quit。
- D16 历史分组 / 保存点和 D18 marker；undo / redo 通过相同编辑入口回放。
- 全局 kill ring 与连续命令链；平台输入提交也打断连续 kill。
- D17 组合屏障、只读目标及 pending 前缀约束，包含直接输入回调路径。

验收：

- 纯键盘基本编辑，前缀无超时；undefined 不插入且正确回显，Meta / Shift / AltGr / 死键分类有断言。
- 对 [Emacs 基准](../tests/emacs/README.md) 逐项比较文本、point、mark / active、history、modified 与 echo。
- 连续输入 / 删除合并，移动打断，kill / yank 分组、redo point、mark 坍缩 / 恢复、undo 后输入均有回归。
- 键位与命令名逐项核对，保留真实命令链路结果；不以空保存命令代替 M2 实际保存验收。
- 补齐 R1 性能测量中的按键到显示更新延迟。

## M2 minibuffer、文件与 buffer 管理

交付：

- `M-x`、`find-file`、`save-buffer`、`write-file`、`switch-to-buffer`、`kill-buffer` 和退出。
- minibuffer 的命令名 / 路径 / buffer 名基础补全，prompt 保护、确认 / 取消与焦点恢复。
- 多 buffer 切换与状态保留；正文、minibuffer 共用 kill ring，历史独立。
- 复用文件层保存格式；失败处理、覆盖确认、dirty buffer 的 kill / close / quit 确认。

验收：

- 不碰鼠标完成打开 → 编辑 → M-x → 保存 → 切换 → 退出。
- A → B → A 的文本、point / mark、滚动、历史和高亮不串用；kill 后迟到回调不污染其他目标。
- minibuffer 基础编辑与正文共用实现，prompt 不可编辑；补全经过 Edit 入口，退出后无残留 pending。
- 组合期间保存 / 切换 / 进入或退出 minibuffer 按 D17 拒绝；关闭 / 失焦按 D19 取消。
  不落盘预编辑，不把取消或迟到提交写入新会话；已有 dirty 内容仍须确认。
- 保存失败不清 dirty、不更新身份或保存点、不破坏原文件；取消确认后内容与焦点保留。
- BOM 有 / 无 × LF / CRLF、末尾有 / 无换行磁盘往返，以及编辑后的格式保留均有测试。
- 保存与 undo / redo 的 modified 行为遵循 D16，磁盘字节独立断言，不用内容比较改写保存点语义。

## M3 v1 保真度与性能验收

逐项核对 v1 的全部键位 / 命令名、mark / region、kill ring、历史、minibuffer、跨 buffer、IME 和退出流程。
mode line 包含 buffer 名 / dirty / 行列，echo 信息与已批准差异有可见说明。
复测 R1 / M1 的性能基线，定位回退，记录实际环境与剩余限制。

验收：范围内 F1 全部通过，范围内 F2 差异有记录，F3 与实际行为一致；
没有未经批准的发布豁免。`M-y`、`C-u`、isearch、split 不属于 v1 条件。

## M4 后续能力

按需求分别决定 isearch、`M-y`、`C-u`、用户配置及 split 的优先级。
如实施 split，先定义 window-point、焦点路由与 `C-x 1/2/3/o` 语义，补同 buffer 多 view 的状态和性能测试。
不因为分离了模型与视图就宣称多 view 已实现。

## M5 diff 与 magit

评估 diff 计算与展示组件，文本显示遵循自有模型及 mode keymap。
magit 风格只读 buffer 复用 section、transient keymap 和命令框架，不另起一套按键 / 编辑语义。

## 测试与完成标准

- 纯单测覆盖文本坐标 / 编辑、marker、history、keymap、kill ring 和文件格式。
- GPUI headless 集成覆盖渲染、真实输入接线、命中与几何；不能替代真实 Wayland / IME 验收。
- 独立 Emacs 基准在 `tests/emacs/`，固定版本、mode、选项、初始状态与 command loop。
- 保存成功 / 失败、覆盖确认和退出安全使用真实临时文件，放 `.pi/tmp/`。
- 每个实现阶段通过 `cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`。
- 本阶段需要的真实环境结果单独记录并经用户同意；待验收时不标记整个里程碑完成。
- 任务进度在 `.pi/tasks/` 维护，临时验证脚本和产物在 `.pi/tmp/`，正式测试不依赖临时目录中的唯一副本。
