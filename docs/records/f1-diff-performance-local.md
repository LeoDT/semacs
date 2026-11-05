# F1 diff 专项性能：本地基线与宿主工具

## 范围与身份

用户确认先做 diff 专项、准备工具并完成本地测试，宿主批次另行确认。本轮未启动桌面窗口，没有 OS 注键、IME 切换或截图；不重跑完整普通编辑历史矩阵，不重开 E3 暂缓项。

受测 HEAD 为 `d6a7ba4`，F1 及本轮工具仍未提交。最终批次 `.pi/tmp/f1-diff-perf-local-02` 保存 HEAD / status、相对 HEAD 的 tracked 完整差异及 untracked 源码、构建日志、隔离配置、Git fixture 与 SHA-256 manifest、逐轮 CSV / spans / stdout / stderr、environment 和 summary。该目录是本地原始产物索引，不是长期契约或待办的唯一存放处。

- 源码补丁 SHA-256：`87fdd8167596e13c5301ea6340901f3dc46109cda6beb203e6021cf49eb240b2`
- release `perf_diff` SHA-256：`85990cea0835d89acdf491c42323f2198dd4c0d8e57b3250cd19e4a352ec5c3c`
- 驱动 SHA-256：`9c5a5e58bca4163f3a69c9bab6b6740d109f556572cc024a87bea9724b87f6a4`

此后仅更新文档 / 本地清单，不将这些文档差异误判为受测产品或工具变更。早期 `.pi/tmp/f1-diff-perf-local-01` 保留；其投影 oracle、报告检查及长行导航尚未强化，不作为最终版本证据，也不与最终批次直接做性能对照。

## 方法与边界

Linux x86_64，内核 `7.2.11-arch1-1`，release + `native-profiler`，TestPlatform / TestTextSystem，窗口请求 960×640，单 view 正文实测 944×576；左右分屏共享 diff buffer。没有真实平台字体或 GPU 测量。

两档确定性仓库均保留不同 HEAD / index / worktree；每文件三处修改，正文含中文及约 4 KiB 长行。规模按实际 patch 而非原文件体积：

| 样本 | 文件 / hunk | staged patch | unstaged patch |
|---|---:|---:|---:|
| small | 16 / 48 | 269,712 B，496 行 | 269,904 B，496 行 |
| large | 128 / 384 | 2,157,696 B，3,968 行 | 2,159,232 B，3,968 行 |

每档三轮独立进程，每轮顺序测 staged / unstaged，最终共 144 行。正式命令路由打开、定位首 hunk、折叠 / 展开、导航到第三 hunk 的长改动行、滚动、左右分屏、再次定位 / 折叠、共享刷新 / 展开及取消分屏。确认长行确已可见并取得精确 shaping 几何。headless 不包含原生 idle 行，不把主动调用 draw 推断为原生无 draw。

每阶段核对目标 mode / comparison root、完整独立正文 oracle、共享 snapshot、布局 display 身份、独立生成的展开 / 首 hunk 折叠行范围及两个 view 的实际投影。刷新要求同一 buffer 的新正文版本，且保留折叠；本轮是相同磁盘内容的重读，不是外部编辑造成的 patch 变化基线。首次打开与刷新分开，文件缓存不受控，不宣称冷磁盘。进程内 staged 首开会发现仓库；后续 unstaged 可复用 comparison，不能把两者差异解释为 staged 固有开销。

所有新 diff 时延只作基线，不套普通编辑冷打开 / ready / 单键门限。headless 首次目标观测在 draw 返回后的采样点，不是原生提交；两侧 ready 只在同一观测中共同满足时完成。计时后的正文 / 投影断言包含在 observed elapsed，不回填 first / ready。spans 有额外观测开销，嵌套区间不能相加。

## 最终三轮结果

以下为各轮最小–最大墙钟毫秒，不以平均值掩盖波动。打开为单 view 就绪；刷新 / 分屏为同场景全部可见 view 就绪。此 TestPlatform 路径中首次目标、选中 ready 与全部 ready 在同次 draw 后观测到，不据此推断原生也同时完成。

| 样本 | 比较 | 打开 | 双 view 刷新 | 分屏就绪 |
|---|---|---:|---:|---:|
| small | staged | 32.346–32.749 | 12.320–12.553 | 4.073–4.379 |
| small | unstaged | 11.529–11.723 | 12.243–12.330 | 3.993–4.031 |
| large | staged | 46.266–46.818 | 25.886–27.054 | 5.351–5.557 |
| large | unstaged | 44.325–45.506 | 45.728–46.031 | 5.471–6.744 |

- small 折叠 / 展开全部观测最大 1.438 ms；large 最大 2.329 ms。
- 长行导航 small 最大 1.445 ms，large 最大 1.574 ms；垂直滚动分别最大 0.438 / 0.373 ms。
- 分段 spans 范围：

| 区间 | small | large |
|---|---:|---:|
| `diff-git` | 10.191–10.383 | 20.219–40.667 |
| `diff-parse` | 0.100–0.161 | 0.731–1.660 |
| `diff-render` | 0.224–0.843 | 1.308–3.101 |
| `diff-install-and-invalidate` | 0.094–0.280 | 0.308–0.883 |
| `section-projection` | 0.001–0.012 | 0.005–0.052 |

`diff-git` 是生产 patch 读取路径，包含进程创建、输出读取线程和 10 ms 进程状态轮询，不是裸 Git 执行时长；不包含首开的仓库发现。端到端打开包含发现及安装 / 激活 / 布局等未单列成本，不能通过相减或相加上述表格推出精确归因。当前没有需要据此启动的产品优化或数值预算变更。

## 工具与检查

`perf_diff.py` 固定构建目录并摘要实际 probe，冻结整个 fixture / oracle / Git 元数据，逐轮前后核对源码、二进制和配置；失败保留部分报告并停批。原生复用现有 profiler journal 和同帧 paint 配对，无软件降级；正文提交、选中 ready 和同场景双 view ready 分开记录，既有 draw ≤16 ms、滚动首次提交 ≤4 ms、30 tick idle 无 draw 保留。其他新增 diff 动作无数值门限，30 秒阶段观测 / 父进程 watchdog 只是安全终止界限。

本阶段进行一次集中审查，补齐独立折叠 oracle、固定 target 目录、HEAD 相对源码归档和报告关键字段 / 身份交叉校验，并让 fish 新模式交由驱动构建 / 归档实际 probe，不依赖普通 semacs 摘要。后续由主代理自检，没有追加 reviewer。

最终检查均通过且无跳过：

- 默认 `cargo test --all-targets` **560 项**；profiler **622 项**，其中新增共用观测 6 项单测及 1 项完整 headless 工作负载测试；共用观测在两个 example 测试目标分别编译。
- 两套全目标 Clippy、fmt / diff 检查及 fish 语法检查通过；release 原生 example 构建通过，但没有运行窗口。
- `tests/test_*.py` **95 项**，其中本轮驱动 / fixture / 报告 **14 项**。
- 末轮并行全目标测试曾暴露旧 `Repository` 仅用 PID + 计数、`create_dir_all` 复用测试目录的隔离缺陷，造成共享 Git 配置 / 目录竞争；失败日志保留在 `.pi/tmp/f1-perf-{default,feature}-tests.log`。已改为时间戳 + PID + 计数及拒绝复用，最终串行完整两套自检通过，见 `*-final-tests.log`。没有修改产品 Git 行为或调整性能预算。

## 结论与下一步

两档三轮的本地 CPU 专项采集、正文 / 投影 / 同场景共享 view 校验均通过，获得基线；**不是新增 diff 性能预算通过或原生验收通过**。

待用户具体确认：small 三轮，每轮一个原生窗口，内部左右分屏，顺序覆盖 staged / unstaged、折叠 / 导航 / 长行可见 / 滚动 / 刷新及 idle；隔离配置与临时 Git 仓库，无 OS 注键、IME 切换或截图。入口见 [验收流程](../acceptance.md#f1-diff-专项性能)。

原生字体 / CPU 提交、横向滚动专项时延、改宽 / 字体、其他分屏方向、更大容量 / 接近资源上限、patch 变化刷新、长期持续使用、真实 IME 时延和 GPU / 屏幕呈现未测；不自动扩测，E3 暂缓问题保持不变。
