# JS 脚本扩展与热重载调研（待确认）

> 本文是调研记录，**不是已批准的架构决策、实施规格或行为契约**。文中的分层、不变量与建议均待用户确认；现有有效边界仍以 [D26](../decisions.md#d26-rust-核心与脚本扩展)、[D28](../decisions.md#d28-buffer-mode-与结构化显示) 和[阶段 D 契约](../emacs-fidelity.md#阶段-djs-配置与文本变换)为准。本文不表示开始实现，也不改变当前阶段顺序。

## 摘要

- semacs 现有「配置快照 + 代次运行时 + 原子替换」已经覆盖纯声明与同步纯变换；扩展的难点是**谁拥有比一次命令活得更久的东西**。
- LSP / 编译任务是**深 L2 / L3** 场景，验证长期服务、事件流与任务所有权；Magit 是**宽 L1 + 轻 L2** 场景，验证 buffer 投影、section、mode 与工作流组合。两者不能互相替代验证。
- 共同原则：**JS 决策，core 执行**。数据、纯函数与 handler 引用可以进 reload 快照；会话、进程、任务、不可逆效果由 Rust 拥有。

## 调研范围与证据边界

- 对象：semacs 当前脚本实现（F1 完成后工作树）与 `~/ghq/github.com/earendil-works/pi/packages/chord/` 的插件架构文档。
- 方法：静态阅读源码与设计文档，对照现有 reload 语义做场景推演；未运行新实验、未做宿主验证、未修改产品代码。
- 不把 Chord 的设计结论直接当作 semacs 契约；Chord 的复杂度背景是多进程、多 host、第三方代码、持久化与版本协商，semacs 是单进程、单用户、单入口配置。
- 本文只记录可复用的判断，不重复 D26 / D28 / 阶段 D 已有正文。

## 1. 现状基线

当前 semacs 的脚本模型可以概括为「一份配置快照 + 一个代次运行时」：

- `init.js` 在专用 worker 中执行，产出 `Configuration`：命令注册表、正文键位、mode 键位、字体、主题。见 `src/scripting.rs`、`src/scripting/runtime.rs`。
- 宿主只暴露配置期声明 API；命令是同步的 `(text) => string` region 变换。配置期 API 在命令执行阶段关闭。
- reload 流程：旧配置继续服务 → worker 构建并校验新配置 → 主线程 `apply_configuration` 整体替换 → 失败保留旧配置。见 `src/app/scripting.rs`。
- 结果隔离：请求绑定 view、buffer 文本版本、输入会话、region / point 与配置代次；成功重载后旧结果不再提交。见 `src/editing/transform.rs`、`src/view.rs`。
- 资源边界：源码 1 MiB、配置 500 ms、命令 100 ms、内存 32 MiB、栈 256 KiB、文本 256 KiB、命令 256、键位 1024、worker 请求队列容量 1。见 `docs/scripting.md`。
- 尚未拥有长期资源：没有文件、网络、进程、定时器、buffer 创建、mode 注册、section 构造、事件订阅 API。

结论：现有模型对「纯数据声明 + 同步纯变换」已经具备完整的 reload 保证；难点不在当前实现，而在扩展后**谁拥有比一次命令活得更久的东西**。

## 2. 分析方法

单一「能力分层」不够用，因为一个真实功能通常跨层。建议用三个正交问题定位每个原语：

| 轴 | 问题 | 影响 |
|---|---|---|
| 所有权 | 它比一次命令活得久吗？ | 决定归 core 还是归脚本代次 |
| 效果 | 可逆吗？需要原子提交吗？ | 决定是纯值、缓冲编辑、外部副作用还是持久状态 |
| 时序 | 同步热路径、异步完成、事件流还是长任务？ | 决定调用模型、取消与背压 |

### 2.1 原语层级（不是功能层级）

| 层 | 典型能力 | reload 保证 | 需要的机制 |
|---|---|---|---|
| **L0 声明式** | 命令名、键位、主题、字体、server 配置、任务定义 | 全量校验后整体替换；失败保留旧 | 现有模型 |
| **L1 有界同步编辑 / 查询** | 读 point / region / 行、message、受限插入删除、创建只读 buffer、构造 section | 单次事务；写带目标与文本版本围栏；旧代结果不提交 | host-call + effect fence + 批量投影 API |
| **L2 异步与外部资源** | 文件、进程、网络、定时器、长任务、LSP 请求 | 资源属于代次；重载时取消 / 有界 drain / 确定性关闭；迟到结果三重围栏 | generation abort、资源注册表、异步命令状态机、core-owned job |
| **L3 事件 / 贡献** | on-save、after-edit、mode hook、补全源、装饰、自定义 LSP handler | 贡献是 registry 条目，随 contributor 存活；commit 后旧回调不再调用；缺失可 fallback | core-owned registry + 贡献 lease + 逐回调隔离 |
| **L4 持久状态 / 多来源** | 跨 reload 状态、版本迁移、多脚本源、第三方包 | 状态归 core，按 namespace / version 持久化与迁移；按来源独立重载 | 版本化文档、来源身份、信任边界 |

一个功能会组合多层。例如 LSP 主体是 L2 服务 + L3 事件面 + L1 效果面 + L0 声明；Magit 主体是宽 L1 + 轻 L2。不能因为功能复杂就新造一层。

## 3. 建议的 reload 不变量（待确认）

以下是把现有语义推广到扩展场景的最小集合，不是新增产品承诺：

1. **声明与激活分离**：配置阶段只声明；资源由 core 在 commit 后创建。配置脚本不产生外部副作用。
2. **generation 所有权**：脚本可见的句柄、任务、订阅都属于某一代；重载时由 core 确定性回收，不依赖 GC / finalizer。
3. **快照进、请求出**：脚本不持有可变核心状态；读取拿快照或句柄，写入提交 effect 描述，由 core 校验并应用。**JS 是决策层，不是执行层。**
4. **commit 是主线程的短临界区**：不可失败、不可阻塞、不可 `await`；昂贵与可失败工作全部在 worker 中预先完成。
5. **错误四分类**：
   - 构建 / 校验失败 → 保留旧配置；
   - commit 后激活失败 → 停用该贡献并报告，不让整次重载回滚；
   - 运行期失败 → 只影响本次调用，隔离并报告；
   - 旧代在途结果 → 按 generation + 目标 + 版本丢弃。
6. **贡献必须有 fallback**：脚本 handler / renderer 缺失、失败、超时或正在 reload 时，core 能退回原生行为。
7. **不可逆效果必须由 core 拥有**：任务、进程、workspace edit、文件写入不能只存在于 JS continuation 中；JS 消失后，已开始的 core 工作仍能完成或被明确取消。

### 3.1 重载事务顺序（建议）

1. 标记 reload 进行中（同一时刻至多一个）；旧配置继续服务。
2. worker 构建新代，完成全部声明与可预先完成的校验。
3. 主线程 commit：先摘旧贡献，再挂新贡献；更新声明、handler、键位、字体、主题；清 pending 前缀。
4. commit 后：只为声明发生变化的长期服务重启；旧代资源确定性关闭；旧代 continuation 丢弃。
5. 报告：失败原因、被停用的贡献、被重启的服务。

### 3.2 必须显式回答的问题

每个新增能力都应能填出一张「重载行为矩阵」：成功重载 / 失败重载 / 旧代在途时，对象分别留下、重建、丢弃还是报告。Chord 的 “What a user sees during a reload” 表是直接参照。

## 4. Chord 可借鉴与不借鉴

### 4.1 可借鉴

| Chord 原则 | semacs 的对应 | 建议 |
|---|---|---|
| facet 只声明，host 激活 | 配置阶段只声明，core commit 后建资源 | 现在采用 |
| staging → commit → 对象 fate | reload 构建 → 原子替换 → 每个对象明确 fate | 现在采用 |
| 贡献在 contributor 存活期间存在，重载整体替换 | command / keymap / mode / handler registry | L3 之前采用 |
| 使用点隔离异常并 fallback | 每个脚本调用点 catch、限时、退回原生 | 扩展时采用 |
| native 资源不跨代迁移，必须重启 | 进程 / 连接 / 句柄属于代次 | L2 采用 |
| 需要跨 reload 的状态放在插件之外 | point / scroll / folding / 诊断缓存归 core | 现在采用 |
| 显式 reload，watcher 只标记不应用 | `M-x reload-configuration` | 已一致 |
| ctx 取消 + owned jobs + 有界输出 | 异步请求、编译任务、输出流 | L2 采用 |
| 不可变快照 + 批量提交 | JS 生成整份 buffer / section 描述，core 安装 | L1 采用 |

### 4.2 不借鉴

- 多 host、link、principal、wire、Durable、对象引用图谱、compose 图：semacs 没有这些失效面。
- 完整 `documents` / `watchable` / `derive` 体系：编辑器状态由 Rust 核心直接提供原语即可。
- per-plugin loader 与 per-plugin keep-old + closure rule：当前单文件配置继续「整份成功或整份保留旧配置」，更简单可预测；只有出现多来源时才需要重新评估。
- async-everywhere：code action 过滤、inlay hint 开关等需要同步快路径，不能所有 API 都变 Promise。

## 5. 场景 A：Rust LSP 特殊部分与编译任务

前提：LSP client（进程、JSON-RPC、文档同步、诊断缓存、请求路由）已在 semacs core。脚本只处理 Rust 特有部分。

### 5.1 能力归属

| 子能力 | 所有权 | 效果 | 层 | 重载时 |
|---|---|---|---|---|
| server 定义、初始化参数、root 规则 | 脚本声明数据，core 建会话 | 无 | L0 | 声明未变不重启；变了才重启并重放 open docs |
| 自定义请求 / 通知 handler | 脚本函数，core 会话 | 读或 effect 描述 | L2 | 旧 continuation 丢弃；core 已拥有的效果继续 |
| workspace edit / code action 应用 | core 事务 | 多文件文本 | L1 | 版本围栏，原子提交或整体拒绝 |
| 诊断 / status / progress 展示 | core 状态；脚本可选纯格式化 | 显示 | L3 | 订阅随代次替换；高频流用 latest-value |
| inlay hints / 装饰 | 脚本贡献，core 应用 | 显示 | L3 | 旧贡献移除，新代按文档版本重取 |
| 编译任务 | core job | 外部不可逆 | L2 | 任务继续；结果记录，新代可查询 |
| 任务输出解析 | 优先 core（cargo JSON）；脚本只做纯函数 | 显示 / 诊断 | L1 / L3 | 纯函数整体替换，不中断输出 |
| 每项目设置 / 任务历史 | core 状态 | 持久 | L4（暂缓） | core-owned 才保留 |

### 5.2 脚本能力分三类

- `declare`：server 配置、任务定义、设置。纯数据，可比较 / 可 diff。
- `decide`：状态文本、code action 过滤、自定义输出解析。纯函数，限时，失败 fallback。
- `request`：发起 LSP 请求 / 启动任务。返回 effect 描述或 job id；**continuation 与效果归 core**。

### 5.3 关键结论

- 这是**深 L2 / L3 场景**：长期服务、双向调用、事件流都在这里出现。
- 最难的不是 API 数量，而是所有权：LSP 会话、诊断、任务进程必须在 core，否则一次 reload 就会杀掉会话或丢失已产生的状态。
- handler registry 需要 `method / command → generation-tagged fn`，每个来源至多一条，冲突报告，原生 fallback。
- 高频诊断 / 进度不能逐事件进 JS 队列；应使用 latest-value 快照，日志 / 任务输出使用有界流。
- 声明需要等价性判断（如配置 digest），避免每次 reload 都重启所有 server。
- 在途请求：core 请求可继续；JS continuation 随代次消失；已提交的 effect 由 core 保证完成。
- 运行中的编译任务默认不应因 reload 被杀；完成回调不是唯一结果通道，core 记录结果供新代读取。

### 5.4 当前缺口

- 无 async / Promise 支持；出现 pending job 会隔离整个运行时直到重载。
- 无进程、输出流、任务所有权 API。
- 无 core → JS 事件路由与 generation lease。
- 无 handler registry 与 effect 描述协议。
- 无声明 digest，无法判断 server 是否需要重启。
- 无每项目 / 每窗口的会话身份模型。

## 6. 场景 B：Magit

### 6.1 引擎与工作流分离

| Magit 组成 | 建议归属 | 依据 |
|---|---|---|
| git 执行、仓库发现、版本读取 | Rust core | `src/git.rs` 已有读取、超时、取消基础 |
| diff 解析、渲染、版本访问 | Rust core | F1 已实现并有性能预算 |
| section 树、折叠、可见行、导航、anchor 恢复 | Rust core | `src/section.rs` 的 `Sections::new` / `restore` / `anchor` / `resolve` |
| buffer 投影、刷新、只读约束 | Rust core | 产物比脚本代次活得久，必须 core 拥有 |
| mode 身份、局部键位、命令注册 | core registry，JS 声明 | 当前 mode 是固定集合 |
| status 布局、section 组合、标签格式 | JS | 纯数据 + 纯函数 |
| 命令编排、动作选择、确认流程 | JS | 组合层 |
| patch 构造、apply / unstage / discard | Rust core | 结构化选区、二进制 / rename / 部分 hunk、失败恢复 |
| transient / suffix 菜单 | Rust core（当前没有） | UI 子系统，不是脚本能力 |
| 刷新编排、错误展示、用户定制 | JS | 短流程 |

### 6.2 「没有存活时间问题」的准确含义

Magit 没有 LSP 那种长会话，但存在**产物寿命**：buffer、section、mode、keymap、折叠状态都比脚本代次活得久。

- 如果 buffer 是「核心 Git 快照的投影」，reload 只需要重新投影，point / scroll / folding 由 core 保存，难度低。
- 如果 script 拥有 buffer 内容与 section 语义，reload 就退化成数据迁移与 schema 兼容问题。

因此建议把 Magit buffer 定义为**投影，而不是脚本拥有的文档**。这是该场景对架构最重要的约束。

### 6.3 层级定位

Magit 是**宽 L1 + 轻 L2**：

- L0：mode 定义、局部键位、命令名。
- L1：只读 buffer、批量写入、section 构造、刷新替换、结构化选区。
- L2：短命的 git 进程与异步刷新。
- L3：mode / 命令 / 动作注册与 fallback。
- 基本不需要 L4：Git 是唯一真源。

### 6.4 当前缺口

1. `ModeId(&'static str)` 与固定 `MODES`，无法注册 JS mode；需要可扩展注册表、稳定 id、旧 mode 消失后的 fallback。
2. 命令模型只有 region transform，没有「命令 → effect」模型，也没有异步。
3. 没有 buffer 创建 / 批量写入 / 只读 buffer API；`Buffer::named`、`from_text`、`set_mode` 均为 `pub(crate)`。
4. `Sections::new` 已有批量构造与校验，但无脚本入口。
5. 没有通用 process API；`git::read` 是 crate 内部且专用。
6. 没有 transient / suffix UI；minibuffer 只能覆盖单值读入。
7. 没有 JS 范围样式 / 装饰 API（E3 已明确未开放）。
8. 脚本运行时异步即隔离，无法承载 git 调用。

### 6.5 适合作为「完整 JS 扩展」的驱动场景吗

- **适合作为广度 / 集成验收场景**：外部真源、可重建、无长会话、有真实价值、可增量切片。
- **不适合作为第一个驱动场景**：一次性需要 mode、buffer、section、process、async、transient 五六类原语；API 面过宽；性能敏感；与 F1 已有实现重叠；且不验证最难的服务生命周期问题。
- 若目标是尽快交付 Magit：native 引擎 + JS 工作流风险最低，符合 D26。
- 若目标是验证脚本能力：Magit 适合在 L1 / L2 原语就位后作为集成场景，而不是用来定义架构。

## 7. 跨场景结论

| 场景 | 深度 | 主要所有权 | 重载难度 | 验证重点 |
|---|---|---|---|---|
| LSP 特殊部分 + 编译任务 | 深 L2 / L3 | core 拥有会话与任务 | 高 | 长期服务、事件流、双向调用、任务存活 |
| Magit | 宽 L1 + 轻 L2 | core 拥有投影与 section | 中低 | buffer / mode / section / effect / 广度组合 |

三个结论：

1. **LSP 与 Magit 不能互相替代验证。** LSP 验证生命周期与事件，Magit 验证组合能力与投影模型。
2. **共同原则是「JS 决策，core 执行」。** 能放进 reload 快照的只有数据、纯函数与 handler 引用；必须完成的工作由 core 拥有。
3. **原语应按实际场景递增，而不是先建完整插件框架。** 调研中较自然的顺序是：先固化不变量 → L1 的 buffer / section / mode / effect → L2 的 async / process / job 所有权 → L3 的 registry 与事件 → L4 仅在出现真实需求时评估。

### 7.1 低风险、可先做的整理

- 把第 3 节不变量写成文档，作为后续 API 的准入条件。
- 保持单文件配置「整份成功或整份保留旧配置」，不提前引入 per-source keep-old。
- 不把 `ScriptCommand { generation, index }` 的索引身份泄漏到未来 API；多来源前改为 source id + 稳定名字。
- 不逐个 API 打补丁；L2 需要一次性设计 generation 所有权、取消与回收。

## 8. 待确认事项

以下问题会直接改变架构，本文只记录，不替用户决定：

- LSP 会话归属：app 级、window 级还是 project 级。
- 编译任务是否跨 reload 存活；默认建议是。
- 诊断 / 任务输出由 core 原生处理，还是允许脚本可插拔解析。
- 是否准备支持多脚本来源 / 多文件配置；决定 registry 与 per-source 重载时机。
- transient / suffix 是否进入产品范围；它属于核心 UI，不是脚本能力。
- Magit 引擎是否确定留在 Rust，只把工作流交给 JS。
- 是否采用「effect 描述 + core 应用」的统一协议，还是为每类能力单独设计接口。

## 9. 参考

- semacs：`src/scripting.rs`、`src/scripting/runtime.rs`、`src/scripting/bootstrap.js`、`src/app/scripting.rs`、`src/command_registry.rs`、`src/keymap.rs`、`src/mode.rs`、`src/buffer.rs`、`src/section.rs`、`src/diff.rs`、`src/git.rs`、`src/editing/transform.rs`。
- semacs 文档：[D26](../decisions.md#d26-rust-核心与脚本扩展)、[D28](../decisions.md#d28-buffer-mode-与结构化显示)、[JS 配置](../scripting.md)、[阶段 D 契约](../emacs-fidelity.md#阶段-djs-配置与文本变换)、[F1 契约](../emacs-fidelity.md#阶段-f1mode-与只读-magit-diff)、[Magit 源码调查](f-magit-reference.md)。
- Chord：`packages/chord/docs/design.md` 的 §16 facet、§17 registries、§21 reload and rollout、§15 jobs、§21 插件模型；`packages/chord/README.md`。
