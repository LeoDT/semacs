# 技术决策

本文件只列当前生效的选择。行为要求见 [Emacs 保真度](emacs-fidelity.md)，实现门禁见 [实施计划](plan.md)。
编号用于文档引用，不表示对应能力已经实现。当前有文件层、R1 文本 / 输入模型及最小平台编辑驱动；R1 的功能证据、headless / 原生预算及验收边界已确认，R1 已完成；完整 Emacs 命令与历史（M1）、保存（M2）尚未实现。

## D1 依赖与构建

- Rust 2024 edition；依赖版本由 `Cargo.lock` 固定。
- gpui-kit 使用 crates.io 的 `0.8.1`，仅用于外围 UI 和主题。
- 使用与 gpui-kit 兼容的同一套 GPUI；当前依赖解析为 `gpui-pre 0.3.10`，不混用另一套 GPUI 类型。
- dev 依赖为同一 gpui-kit 启用 `test-support`，用于 TestPlatform / TestTextSystem 回归与基线；不切换产品平台或混入另一套 GPUI 类型。
- 本地参考仓库路径不进入构建配置；平台依赖调整先确认，不直接修改 Cargo registry 缓存。

## D2 平台

Linux 优先，Wayland 为主要验收环境；X11 的启动和基础输入是兼容目标。
GPUI 提供窗口、平台事件、输入法协议、字体系统与绘制；semacs 的平台输入分类和编辑器输入适配集中管理。
这些是职责与验收要求，不表示所有平台已经完成实机测试。

## D3 键位

使用纯 Emacs 键位，不并存 CUA 的 `Ctrl-C / Ctrl-V`。`C-c` / `C-x` 是前缀，复制 / 粘贴走 `M-w` / `C-y`，`C-v` 为翻页。
系统保留键由平台路由处理；编辑目标内其他未定义命令键消费并回显，不插入字符。

## D4 命令分发

自有 keymap、无超时前缀和命令解析接管编辑目标的键事件，GPUI action 不作为主命令系统。
自插入和 IME 回调也受目标、前缀及只读约束，不因为绕过按键事件就绕过语义入口。
顶层 `C-g` 停用 region、保留 mark 并回显 `Quit`；minibuffer 中取消会话。
`C-x C-g` 按当前 keymap 报 undefined、清空 pending，保留 mark / active。

## D10 工程结构

单 crate，按文本、Buffer、view、输入、命令等职责划分模块；不预建通用插件或可插拔后端框架。
选择性导入的纯组件在本仓库维护，不要求建立独立发布工程。

## D11 命令注册表

命令名、说明和执行入口使用唯一注册表，供 keymap、`M-x` 与后续帮助查询共用。
采用 Emacs 命令名；随实现逐项与参照绑定核对，不以存在导出脚本代替全表核验。

## D12 外观

不模拟 Emacs 外观。外围 UI 使用 gpui-kit，编辑区使用自己的视图及 GPUI 排版 / 绘制能力。
mode line 至少提供 buffer 名、dirty 和行列信息；echo area 展示前缀、错误及命令消息。

## D13 键位配置

v1 默认键位写在 Rust 中；用户配置格式与覆盖机制另行设计，不阻塞 v1。

## D16 编辑历史与保存点

各正文 buffer 和 minibuffer 的历史独立，分组与 modified / 保存点由自有模型控制：

- 连续同类自插入、连续后删、连续前删分别合并；移动与其他命令插入边界。
- 连续 `kill-line` 各自成组；kill ring 的追加不等于历史合并。连续 yank 也是独立单元。
- undo / redo 不与输入合并；undo 后立即输入开新组并清空 redo。
- `kill-line` 在 EOB 报 `End of buffer` 且不建组；yank 将 mark 置于插入起点并回显 `Mark set`。
- undo 恢复组开始前的 point；redo 回到组内最早变更起点。
- 回到保存点为 clean；已固定的保存后 undo 越过保存点场景仍 clean，再输入为 dirty。
  磁盘内容单独验证，不用当前文本等于磁盘内容重新定义 modified。
- region 激活时执行全局 undo；不实现 undo-in-region 和 Emacs undo 环，按 F3 登记。

历史回放经过唯一 Edit 入口，但不重复记录为新用户编辑。完整分组、marker 回放和保存点算法在 M1 实现，M2 验收实际文件保存。

## D17 IME 命令屏障

当前目标存在未提交预编辑时，不执行编辑器命令、不启动前缀，回显：
`Input method composition is active; finish it first`。
IME 自己消费的候选操作不作为编辑器命令处理；用户完成组合后重新触发命令。
该策略是批准的交互选择，不随实现能力自动改变。

## D18 mark 与 marker

mark 与 region 激活状态分离；停用 region 不删除 mark。所有已提交文本变更在唯一 Edit 入口更新 marker：

- insertion type nil：在 mark 前插入使其后移，在 mark 处或之后插入不动。
- 删除覆盖 mark 时坍缩到删除起点，记录原塌缩偏移。
- undo 只在 mark 仍位于该变更的删除起点时恢复偏移，否则按文本位移调整；redo 再按删除规则处理。
- 仅 marked 临时文本的更新 / 取消不调整 mark；平台显式替换造成的已提交变更和最终输入提交经 Edit 更新 marker。
- 输入提交、undo / redo 和顶层 `C-g` 停用 region，但保留 mark。

Emacs 参照材料位于 [tests/emacs/](../tests/emacs/README.md)。这些定义是 semacs 的待实现契约，不是产品通过记录。

## D19 关闭与失焦

窗口关闭或失焦时丢弃仍 marked 的临时预编辑，不强制提交、不保存。
普通输入在 point 插入，不把 Emacs region 当作平台替换选区。
平台显式替换已删除的正文不因取消而恢复；该删除经 Edit 生效，可产生 modified。
纯预编辑更新 / 丢弃本身不置 dirty，也不清除已有 dirty。
平台 unmark_text 按解除标记并保留文字处理，与丢弃 marked 不同。
这是用户确认后的收窄约定，取代原先“非空替换取消也完整回滚”的要求；不为此修改 GPUI。
已有 dirty 内容仍需执行关闭确认，取消确认后保留已提交正文和焦点。
外围确认框安全默认 Cancel；Escape / C-g 取消，Tab / 左右键选择，Enter / Space 确认，不改变正文 Escape / Meta 的待决语义。
所有输入绑定目标 / 会话代次，关闭、切换或重开目标后到达的回调不得污染当前目标。

## D20 文件格式

- 支持 UTF-8 有 / 无 BOM、统一 LF / CRLF。内部正文无 BOM、使用 LF；Buffer 保留原始格式元数据。
- 保存恢复原格式，新换行沿用 buffer 格式；不自动增删末尾换行或重复写 BOM。
- 解码再编码未编辑文件须保持字节一致；读取和格式适配不产生历史或置 dirty。
- 无效 UTF-8、UTF-16 / UTF-32 BOM 和含 NUL 的输入明确拒绝，不做有损解码或编码猜测。
- 混合换行和裸 CR 当前明确报错；如要扩展支持，先决定保留 / 转换策略，不能静默归一化。
- 保存失败不清 dirty、不更新文件身份或保存点，不破坏原文件。

读取、格式元数据和编解码已实现；实际保存与覆盖确认在 M2 完成。

## D21 自有 editor

- semacs 拥有文本编辑模型、显示映射、视图和输入适配，以及 buffer / window / mode / command 语义。
- GPUI 负责平台协议、字体 shaping、绘制与窗口；gpui-kit 仅用于外围交互组件和主题。
- Buffer 的已提交文本是权威状态；预编辑为目标绑定的临时状态，命令、平台提交和历史回放经唯一 Edit 入口。
- 文本 / 布局快照和后台任务有版本，输入有目标 / 会话身份；过期结果不覆盖新状态。
- v1 只有一个正文 view，point / goal column 可以归 Buffer；view 拥有滚动、viewport 和布局。
  多 view 的 window-point 与 split 后续单独定义，不提前实现。
- R1 在正式模块中构建最小纵向切片，先验收换行、输入、几何和性能；不另造一次性大型验证应用。

## D22 源码复用与许可

参考来源为 `https://github.com/zed-industries/zed`，冻结快照：
`ba63a022c9a22ca66d68ef2a8f0bb9b5b0a9984d`。

- 首批已导入 `crates/rope`（GPL-3.0-or-later）和 `crates/sum_tree`（Apache-2.0）到 `src/text/`，以及其最小工具函数与纯测试支架。
- 优先调用当前 GPUI 的 shaping / LineWrapper 等 API；按需要从 Zed 提取换行调度、显示映射、命中测试和 IME 几何算法。
- 不整体引入 Zed editor / text、MultiBuffer、language 或 project，不依赖 Zed 运行时，不维护外部 fork。
- 导入前核对实际文件的许可证和版权，保留适用的许可证文本 / NOTICE，记录来源 commit、路径、本地路径、修改范围与测试。
- 用户接受 GPL-3.0-or-later 组件及其对整合作品分发的要求；不能把 GPL 源码重标 MIT。
  原 Apache 组件保留自身许可，自有代码的额外授权不覆盖第三方代码。
- 已补齐 [来源 / 修改记录](../THIRD_PARTY.md)、上游许可文本及 [项目许可 / 分发要求](../LICENSE)。
  Cargo 的整合作品许可为 GPL-3.0-or-later，Apache 文件保留自身许可；二进制发布前仍须审核并附带依赖归属材料与相应源码。
- 按已验证缺陷选择性吸收上游修复，不持续追随 Zed，也不假设通用代码无需维护。

## D23 R1 验收范围与后续测量

- 用户已批准 [性能记录第 9 节](performance.md#9-宿主硬件路径基线与已确认预算) 的独立原生 CPU / 提交流程预算；headless 预算保持不变，后续回归不得调宽预算掩盖回退。
- R1 按真实 Wayland / fcitx5 功能证据（软件渲染）与宿主 RADV 硬件 CPU / 提交流程测量的组合完成验收；用户已明确要求标记 R1 完成。
- GPU 执行时间、实际屏幕呈现及纯实体输入端到端延迟仍未测，作为明确限制保留，不作为 R1 阻塞项；不得重标为通过。
- D17 / 失焦 / 关闭沿用既有真实 Wayland 功能证据，不要求仅因切换硬件渲染器完整重跑。
- 持续输入和完整 Emacs 命令链性能必须在 M1 补测；其他平台 / 输入配置按支持范围后续验证。R1 完成不等于 v1 发布验收完成。
- GPU 内部与实际呈现的专门测量，在出现瓶颈或需要端到端延迟承诺时再决定，不预先扩大 M1 范围。

## 待决事项

- `ESC` / Meta 的完整交互、基础缩进以及相关 Emacs 选项。
- kill ring 与系统剪贴板的同步策略。
- 文件覆盖确认的具体流程和文案；自动保存与备份属于后续增强。
- v1 后的 split、isearch、`M-y`、`C-u` 和用户配置优先级。

新增保真度差异、平台依赖调整或输入策略变动必须确认后实施；真实环境验证另行征求同意。
