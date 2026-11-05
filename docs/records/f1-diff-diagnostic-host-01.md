# F1 diff 完整原生诊断：三轮宿主回传

## 范围与身份

用户明确批准在原 small 三轮范围内完整采集，性能超限记录后继续后续动作 / 窗口，正确性错误、异常及安全超时仍停止；门限不变、诊断非验收。用户运行 `diff-diagnostic` 并回传完成目录 `.pi/tmp/f1-diff-diagnostic-host-01`。本次只离线检查，没有重试、追加窗口或修改原始产物。

受测 HEAD：`d6a7ba4890f30e24b70794d78294535d9239a440`，F1 与工具仍未提交。内层 `diff/` 保存源码、构建、环境、冻结 fixture、三轮 CSV / frames / spans / stdout / stderr 和 summary。

- 源码补丁 SHA-256：`eba34905755433f82f86e29ced8762641b721abbe71f8a886dd0d7ac25add967`
- `perf_native` SHA-256：`f8ffa4e7a45e048b89e354af2b6a2c36efc85e1dcdf9695fedea2c529c065488`
- 驱动 SHA-256：`28a66ab6f88486e8db196b4181280105dc89e10c8f66873d728e7110e3f2b536`
- small manifest SHA-256：`a3de15673bca7487371a847d971f473a6f37ff119facae13f86a0f56547a4374`

离线检查时当前源码补丁与宿主归档逐字节一致，HEAD / status / 补丁摘要、实际 release 二进制及驱动身份一致。冻结的 HEAD / index / worktree、Git 配置 / patch、正文及投影 oracle、manifest 和隔离空配置均核对通过。之后仅更新文档，不将这些文档差异误判成受测代码变化。

## 环境与覆盖

Linux x86_64，内核 `7.2.11-arch1-1`，Wayland `wayland-1` / DISPLAY `:0`，release + profiler；原生 renderer `is_software_emulated=false`，设备标识 `AMD Ryzen 7 7700 8-Core Processor (RADV RAPHAEL_MENDOCINO)`，radv / `Mesa 26.2.5-arch1.1`。默认请求随包 Lilex 16px / 24px，不把配置请求解释为额外字体命中诊断。

窗口请求 960×640，但宿主实际正文为：初始 setup 744×1312；首次命令准备 739×1218；后续命令准备 739×1242；diff 单 view 739×1306、左右双 view 每侧 361×1306。以报告中的实际几何为准，不套用 headless 或请求尺寸作直接性能对照。

实际 patch：staged 269,712 B、unstaged 269,904 B；各 16 个文件、48 个 hunk、496 行，含中文与约 4 KiB 长改动行。每轮依次 staged / unstaged 各 13 阶段；共 **78 行动作、93 个实际 draw 帧、10,977 条 preparation spans**。三份 stderr 均为空，stdout 阶段序列完整且无 STOP。

独立重读三轮 CSV / frame / span 并重新生成 summary，逐字节对应 JSON 内容；逐帧时间 / 阶段顺序、提交边界上的目标正文版本与投影 / 几何、选择 view、selected / 同场景 all-view ready、完整正文与独立投影 oracle 均通过。无缺失观测或空白正文帧。诊断记录性能超限而没有提前停止，三轮都到达最终 unstaged unsplit。

## 性能结果

下表为三轮最小–最大墙钟毫秒；定位 / 折叠 / 展开在每种比较中含单 / 双 view 的规定动作，不是逐键测量。打开为单 view，分屏 / 刷新为同场景全部可见 view 就绪。本轮 first / selected ready / all ready 在同一份目标提交观测到，不推断其他规模也一定同时完成。

| 动作 | staged ready / submit | unstaged ready / submit |
|---|---:|---:|
| 打开 | 62.588–65.099 | 29.511–29.946 |
| 首 hunk 定位 | 3.141–4.349 | 2.940–4.050 |
| 折叠 | 4.573–7.155 | 5.059–7.439 |
| 展开 | 2.369–3.969 | 2.431–3.873 |
| 导航到长改动行 | 2.871–3.983 | 3.395–3.637 |
| 垂直滚动 | 1.090–1.978 | 1.197–1.356 |
| 左右分屏 | 12.388–16.304 | 15.030–16.041 |
| 共享刷新 | 22.525–24.171 | 23.769–24.084 |
| 取消分屏 | 6.574–6.931 | 6.509–6.944 |

新 diff 打开 / 折叠 / 导航 / split / refresh 等没有批准时延门限，仅作基线。分屏 16.304 ms 不是“超过单键 16 ms”，定位 / 导航的多组合键同步分发也不套单次编辑 ≤2 ms。staged 首开还包含仓库发现，后续 unstaged 可复用 comparison；不将两者差异全部归为 Git 比较类型。

适用的既有动作预算：

- 所有 diff 测量动作单次 CPU draw 最大 **14.731883 ms ≤16 ms**。
- 垂直滚动首次目标提交最大 **1.977737 ms ≤4 ms**。
- 六段 idle 各采满 30 tick，总计 **180 tick、0 draw**，不据此填造 0 ms 帧或功耗。
- 上述已覆盖项目满足相应预算；不是完整性能或 GPU / 实际呈现验收。

### 唯一重复超限：首次命令准备帧

三轮各有且仅有一个超限帧，均为 step 0 的 `command-prepare`：自动流程已经通过 M-x 输入完整 `magit-diff-staged`，尚未确认执行。该帧仍在显示 24 B 的普通源 buffer，`target_kind=other`；没有开始显示 diff 正文。

| 轮次 | CPU draw | request-layout | prepaint | paint | other | CPU submit |
|---|---:|---:|---:|---:|---:|---:|
| 1 | **35.407938** | 0.034870 | **34.355095** | 0.987043 | 0.030930 | 3.313928 |
| 2 | **37.826702** | 0.048920 | **36.639669** | 1.102673 | 0.035440 | 2.853921 |
| 3 | **37.497094** | 0.043100 | **36.240352** | 1.005604 | 0.208038 | 2.412814 |

draw 三轮均 >16 ms，不能以平均值或后续快帧掩盖。主要成本在 prepaint（约 34–37 ms）；本批窗口初始 source draw 仅 0.184–0.259 ms，第二次 M-x（准备 unstaged）draw 1.184–1.213 ms。

因此峰值已定位到**自动流程首次命令界面准备帧的 prepaint 阶段**，不是本批 diff Git 读取 / 解析 / 安装阶段；但还不能定位到具体 view / shaping / 字体或缓存根因。当前逐帧表没有该 setup 阶段更细的 view / preparation spans，不能凭首次 / 后续差异直接断言是字体冷缓存。它也不是用户真实逐字 OS 输入的端到端延迟测量。

### diff preparation 基线

三轮所有对应区间的最小–最大值（ms）：Git patch 读取 10.230–10.533，解析 0.216–0.394，正文 / section 生成 0.362–1.151，安装 / 失效通知 0.100–0.220，section projection 0.002–0.012。Git 读取包含既有进程 / 线程与 10 ms 状态轮询，不是裸 Git 执行时间；首次仓库发现未单列。区间可能嵌套，不能相加解释整帧或端到端打开。

## 结论与下一步

三轮采集完整，身份 / 正文 / 投影 / 帧与报告交叉核对通过；已覆盖的 diff 动作 draw、滚动与 idle 满足适用预算，新 diff 时延取得原生基线。**总体 `budget_passed=false` 的原因是三次首次命令准备 draw 峰值；`diagnostic=true; acceptance=false`，不记作整体验收通过。**

建议下一步由用户确认是否针对首次命令准备 prepaint 做定向定位，而不是继续盲改 diff 或扩完整矩阵。本轮未启动优化、追加观测或重测；该峰值尚未被用户接受延期，也不能自动套用 E3 字体切换峰值的暂缓决定。

仅覆盖 small、左右共享双 view 及不变 patch 刷新。更大规模、其他方向、patch 变化刷新、横向滚动专项时延、改宽 / 字体、持续输入 / 真实 IME、GPU 执行 / 屏幕呈现未测；无人工视觉结果，不将自动几何与正文检查外推为视觉功能全部验收。
