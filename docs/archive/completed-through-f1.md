# 已完成阶段归档（R0–F1）

本文从整理前的 `docs/plan.md` 迁出已完成阶段摘要，保留当时的交付结论、测试数量和未测边界，不追随当前代码更新，也不表示所有性能预算通过。各批次原始证据仍在 [冻结记录](../records/README.md)，不因本次归档改写。

当前工作和所有仍需跟踪的缺陷、暂缓项以 [实施计划](../plan.md) 为唯一入口；本页不是待办或有效行为契约。后续阶段完成时按实际需要另建归档，不继续向本页累积。

## 已完成阶段

- **R0**：产品范围、组件选择、许可边界、应用外壳与文件读取基础。
- **R1**：最小正式编辑切片及输入 / 显示性能基线，见 [验收记录](../records/r1.md)。
- **M1**：键位、共享编辑语义与历史，见 [验收记录](../records/m1-performance.md)。
- **M2 / M3**：minibuffer、文件 / buffer 管理及既定 v1 保真度 / 性能收尾，见 [验收记录](../records/m2-m3-local.md#阶段最终结论)。

### 阶段 A：显示设置

字体 / 字号设置与方块光标已完成，按既定范围验收通过；固定证据及未测边界见 [验收记录](../records/a-display-local.md)。

### 阶段 B：候选式 minibuffer

已确认的候选交互与评分匹配范围已完成，本地回归及用户宿主定向人工验收通过。证据见 [本地验证](../records/b-minibuffer-local.md) 与 [宿主验收](../records/b-minibuffer-host.md)；后者包含测试前 mark 键位调整及其回归。本轮未复测原生性能，保留记录中的字体 / 尺寸等证据边界，不将功能确认外推为未测矩阵。

后续候选刷新跳动修复已包含于 `738e744`：349 项 Rust 测试及默认 / profiler Clippy 通过，用户已目视确认修复正常。此补充不改写先前冻结验收记录，也不增加原生性能通过结论。

### 阶段 C：buffer / view 与 split

共享 buffer 与独立 view 的状态归属、分屏 / 选择 / 关闭、共享编辑及输入路由已完成，用户确认按已批准范围验收收尾。行为见[保真度契约](../emacs-fidelity.md#阶段-cbuffer-与-view)，本地回归与基线见[初次验证](../records/c-split-local.md)、[自动化 / 多 view 基线](../records/c-automation-local.md)及[原生工具验证](../records/c-native-performance-local.md)；宿主证据见[真实 IME](../records/c-split-host.md)与[定向原生性能](../records/c-native-performance-host.md)。

本轮已覆盖的选中 view 预算及 draw / idle 要求满足；聚合基线与未测范围按记录解释，不扩大为全样本、所有尺寸、持续输入或 GPU / 实际呈现通过。headless 聚合观察按[当前计划](../plan.md#已知问题与未排期事项)的非阻塞边界保留，无剩余必测性能项，不因该观察继续扩测。

### 阶段 D：最小脚本接入

JS 配置、自定义正文键位、统一命令注册与 region 文本变换已完成，用户确认按本轮范围收尾。行为见[阶段 D 契约](../emacs-fidelity.md#阶段-djs-配置与文本变换)，验证及未测边界见[本地记录](../records/d-scripting-local.md)与[宿主冒烟](../records/d-scripting-host.md)。

D 验收时遗留的错误信息与 mode line 重叠缺陷已由下节 D1 处理；不改写 D 的修复前记录，也不扩大为未测性能、真实 IME 或 GPU 通过。

### 阶段 D1：消息显示与历史

窗口级消息状态、有界多行 echo、读入末尾临时注记，以及只读 `*Messages*` / `C-h e` 已完成；本地回归、审查后修复及用户宿主定向验收通过，用户确认按本轮范围收尾。行为见 [D1 契约](../emacs-fidelity.md#阶段-d1消息显示与历史)，证据见[本地验证](../records/d1-messages-local.md)和[宿主验收](../records/d1-messages-host.md)。

本轮已确认长 / 多行错误重叠修复、读入提示及恢复、历史回看 / 复制与正文编辑恢复。精确窗口尺寸未提供，原生性能、真实 IME 和 GPU 测量仍未覆盖，按记录保留；无剩余必测项，不自动追加补测。

### 阶段 E0：基础主题与命名样式

应用级单一主题、用户颜色覆盖、JS 主题配置及 `M-x load-theme` 候选切换已完成，用户确认按本轮范围收尾。正文及外围界面统一使用命名样式；换色只重绘，超长字形行的布局和输入坐标保持可用，不引入 Emacs font-lock / 完整 face 机制。行为见 [E0 契约](../emacs-fidelity.md#阶段-e0基础主题与命名样式)，配置 API 见 [JS 配置](../scripting.md#主题与命名样式)，证据见 [本地验证](../records/e0-themes-local.md)及[宿主定向验收](../records/e0-themes-host.md)。

默认 453 项 Rust 测试、静态检查、受影响的 headless 性能回归及五项宿主功能观察通过。具体窗口尺寸未提供，原生性能、真实 IME 和 GPU 未测，按记录保留，不自动追加补测。E0 收尾时既有 profiler 生命周期自检仍失败，当时不宣称完整 feature 测试通过；后续独立修复见下节工具维护，不改写 E0 冻结证据。E0 收尾轮未启动 E1。

### 阶段 E1：Rust tree-sitter

`.rs` 自动启用的共享后台增量解析、中性主题语法前景色与 paint-only 高亮已完成，本地回归 / 一次集中审查修复及用户五项宿主定向观察通过。行为见 [E1 契约](../emacs-fidelity.md#阶段-e1rust-tree-sitter)，证据见 [本地验证](../records/e1-syntax-local.md)与[宿主验收](../records/e1-syntax-host.md)。

本地 470 项 Rust 测试及静态 / 工具检查通过；既有 headless 指标满足预算，新语法加布局就绪仅作独立基线，不套用旧纯布局 ready 门限。宿主额外 `.ts` 文件已确认为名称笔误；具体窗口尺寸未记录，原生性能、真实 IME 与 GPU 未测，按记录保留，不自动追加补测。旧 profiler 工具失败没有记作 E1 交付解决，而是在下节后续工具维护中单独处理。

### 后续工具维护：M2 profiler 生命周期

已确认旧失败由 D1 的真实只读 `*Messages*` 使“一份 scratch”旧假设失效引起；工具现逐步骤核对 scratch、fixture 与消息历史的身份 / 状态，并同步 Rust CSV 分类和 Python 校验，未知 buffer 与关闭 view 释放检查保持 fail-closed，产品生命周期与性能预算不变。

完整默认 470 项、`native-profiler` 501 项 Rust 测试均通过且未跳过；两套 Clippy、51 项 Python、16 项 fixture 工具、fish / 格式 / 差异检查及 release profiler 构建通过，单次集中审查无待修项。证据见 [本地修复验证](../records/profiler-lifecycle-local.md)。旧失败已从当前问题中移除，历史失败日志继续保留；没有运行原生窗口或性能矩阵，不扩大为原生性能 / IME / GPU 通过。

### 阶段 E2：多语言语法支持

内置语言注册表、通用后台增量解析、JS / JSX / TS / TSX / Markdown / HTML / CSS 支持及 Markdown 围栏、HTML script / style 嵌入高亮已完成，Rust 行为保留。用户确认按批准范围验收收尾；行为见 [E2 契约](../emacs-fidelity.md#阶段-e2多语言-tree-sitter)，技术边界见 [D27](../decisions.md#d27-内置语言与通用语法机制)，实现与 headless 证据见 [本地验证](../records/e2-languages-local.md)。

宿主工具见 [准备记录](../records/e2-host-tools-local.md)。[首批宿主回传](../records/e2-languages-host-01.md) 的原始 RET 工具失败及着色闪烁均保留历史记录；后续已修正独立 workload / 文本 / point 预期，并实现编辑期间按映射保留未改动字节的临时配色，不改产品 newline 或语法 ready 口径。修复阶段默认 493 项 / profiler 533 项、两套 Clippy、受影响 headless 及一次集中审查通过，证据见 [反馈修复本地验证](../records/e2-flicker-fix-local.md)；最终人工工具 25 项、Python 68 项及 fish / 差异检查通过。

[第二批宿主复核](../records/e2-flicker-host-02.md) 的三组闪烁观察、独立正文 / 配置字节检查，以及 Markdown / HTML 各约 256 KiB、各三轮代表性原生 CPU 测试均通过。原生报告共 228 行，身份、文本 / point、pending 输入覆盖及适用门限通过，无缺失 / 空白帧，180 个 idle tick 无 draw；语法整体就绪继续独立于纯布局门限。

人工窗口尺寸未记录；首批 IME 观察不冒充时延测量，8 MiB 多语言全矩阵、长期持续输入、真实 IME 时延、GPU / 实际呈现等未测边界按记录保留。无剩余必修项，不自动追加补测；用户要求本轮仅收尾提交，不进入 F。

### 阶段 E3：文本样式与高亮合成

命名样式的字体强调 / 装饰、Markdown 强调合成、独立范围样式与整行背景、字体敏感布局和输入几何已完成；默认随包 Lilex 四个真实变体及 OFL。全文换行采用按有效字体样式缓存的字素宽度，可见行与输入坐标仍精确 shaping；批准的精度边界见 [E3 契约](../emacs-fidelity.md#阶段-e3文本样式与高亮合成)，配置见 [主题 API](../scripting.md#主题与命名样式)。原始实现 / 一次集中审查及两项修复见 [本地实现](../records/e3-styles-local.md)，字体修复见 [随包字体记录](../records/e3-bundled-font-local.md)。

最终本地检查：默认 **533 项**、native-profiler **582 项**全目标 Rust 测试、两套 Clippy、**135 项 Python**（81 项驱动、30 项 fixture、16 项脚本工具、8 项 split IME 工具）、JS / TypeScript、fish、格式 / 差异与字体许可来源核对通过，无跳过。最终字素换行修复的六类约 8 MiB、各三轮 headless 布局 / 输入与无系统字体真实后端成本证据见 [换行修复](../records/e3-wrap-fix-local.md)；完整显示诊断的工具回归见 [本地工具记录](../records/e3-display-diagnostic-local.md)。此次收尾不追加性能、宿主运行或 reviewer。

[第二批宿主人工复核](../records/e3-host-manual-02.md) 的四组字体 / 样式观察及正文 / 配置字节检查通过。[单 mixed 窗口完整诊断](../records/e3-display-diagnostic-host-01.md) 取得 71 行：open / resize / font ready 为 417.739 / 186.353 / 229.014 ms，滚动最大 2.424 ms，单键提交最大 7.936 ms，32 键约 25–27 ms，无空白帧或 idle draw。用户接受当前速度及暂缓字体切换峰值，并明确要求收尾提交；本阶段按该确认范围完成，不继续性能优化，不进入 F。

此前 [默认字体失败](../records/e3-host-manual-01.md)、[字体解析诊断](../records/e3-font-resolution-host-01.md)、[原生完整布局约 20 秒失败](../records/e3-native-perf-host-01.md) 和 [修复后首屏超限](../records/e3-native-perf-host-02.md) 的证据保持不变，不以新结果覆盖旧失败。字体阶段 draw / 首场景仍超原门限，作为用户接受的暂缓项；中文轻微跳动、自定义缺失字体 fallback 也保留在已知问题。单轮诊断不替代三轮正式验收、未执行的 Markdown 性能批次或其余 8 MiB 矩阵；长期持续输入、真实 IME 时延、GPU / 实际呈现等仍未测，不宣称全部性能预算通过。无剩余必修收尾项，不自动追加测试。

### 日常编辑增强：双向 word kill

已按用户要求补充 `kill-word` / `backward-kill-word`，默认 `M-d` / `M-DEL`；沿用既有词边界、共享 kill ring 与独立撤销组，行为见[编辑契约](../emacs-fidelity.md#kill-ring)。新增 9 项测试，`cargo test --lib --tests` 共 495 项通过。按用户要求仅做单元 / 集成测试，不追加宿主、性能或用户配置运行验证，不进入 F。

### 日常编辑增强：段落移动与导航键

已按用户确认的空白行分段规则补充 `forward-paragraph` / `backward-paragraph`，已按用户反馈修正为双向停在分隔空行的行首，连续空白行作为一组处理；边界与停点规则见 [F2 motion 契约](../emacs-fidelity.md#已登记的-f2-motion-差异)。用户配置增加 Ctrl+方向键、PageUp / PageDown、Home / End 与 Ctrl+Home / End，应用默认键位不变。新增 7 项测试，并消除候选交互测试对固定命令数量 / 名称排序的依赖；`cargo test --lib --tests` 共 502 项通过。按用户要求不追加宿主、性能或真实配置运行验证，不进入 F。

### 阶段 F1：mode 与只读 Magit diff

共享 buffer mode、配置期局部键位与命令上下文、section 可见行 / 折叠显示、真实 staged / unstaged、diffstat / 导航 / 刷新及 RET 适当版本访问已完成。默认 **560 项**、native-profiler **609 项**全目标 Rust 测试、两套 Clippy、**145 项 Python**、JS / TypeScript、fish / 格式 / 差异检查及 release 构建通过；六类约 8 MiB、各三轮普通布局 / 输入 / 双方向分屏 headless 回归通过。一次 reviewer 的 5 项 P2 已修复并加入回归，证据见 [本地记录](../records/f1-magit-diff-local.md)。

[宿主定向验收](../records/f1-magit-diff-host-01.md)的四组用户观察、源码 / 二进制身份、正文 / 配置及 HEAD / index 状态独立核对通过；用户确认“看着都没问题”，只读功能按确认范围完成。该批实际窗口尺寸未记录，未测原生性能；后续性能证据单独如下，不改写旧批次。

专项补充：约 264 KiB / 2.06 MiB 真实 patch 的各三轮 headless、完整正文 / 独立折叠投影 oracle 和同场景双 view ready 检查通过，见[专项本地基线](../records/f1-diff-performance-local.md)。首批原生 Probe 初始化失败及修复见 [host-01](../records/f1-diff-performance-host-01.md)，第二批 setup draw 超限停止、无 diff 动作数据见 [host-02](../records/f1-diff-performance-host-02.md)；两批失败产物保留。后续显式完整诊断工具完成一次集中审查修复，默认 **560 项** / profiler **631 项** Rust、**103 项**驱动 Python、两套 Clippy / fmt / diff / fish 及 release 构建通过，见[工具记录](../records/f1-diff-diagnostic-local.md)。

[small 三轮宿主完整诊断](../records/f1-diff-diagnostic-host-01.md)共 78 行动作、93 个实际 draw 帧，源码 / 二进制 / fixture / 配置、正文 / 投影及提交证据核对通过，无空白 / 缺失，适用 diff 动作 draw / 垂直滚动 / idle 预算满足；新增 diff 时延只作基线。唯一重复超限为首次命令准备 draw 35–38 ms，静态排查后的用户接受决定见[定向记录](../records/f1-command-prepaint-inspection.md#用户确认与收尾)。诊断 `acceptance=false; budget_passed=false` 保持，不冒充总体性能通过。

用户要求记录该观察后收尾提交；无剩余必修项，不追加优化、宿主或 reviewer。原始目录全部保留；真实 IME 时延、GPU / 实际呈现、超大 diff、patch 变化刷新及其他未测矩阵不补记通过，E3 暂缓项保持不变。不外推为完整 Magit 或 Git 写工作流交付。
