# F1 diff 原生性能第二批：setup CPU draw 超限

## 批次与身份

用户重新明确批准原 small 三轮范围，使用新目录 `.pi/tmp/f1-diff-perf-host-02`；第一批启动失败目录保持不变。用户在正常 fish / Wayland 宿主运行 `diff-perf` 后回传 exit 2 / STOP。本轮离线分析没有启动窗口、重试或改动原始产物。

受测 HEAD 为 `d6a7ba4890f30e24b70794d78294535d9239a440`，F1 与工具仍未提交。内层 `diff/` 身份：

- 源码补丁 SHA-256：`d0f94e5c66a4ea5f57a1e7c3bb9bb2e9e2553177ea41be832506e28f530b1ac8`
- `perf_native` SHA-256：`66c7adb2f781b65a4012b020ed7b5ade211327e9a542c0922d4cb95f0d03d357`
- 驱动 SHA-256：`9c5a5e58bca4163f3a69c9bab6b6740d109f556572cc024a87bea9724b87f6a4`
- small manifest SHA-256：`6ce5187bf1cc2814c606cc12b8dce8942ad6930f6b723a81edb302b833d88603`

离线检查时当前源码补丁与宿主归档逐字节相同，HEAD / status / 补丁摘要、二进制和驱动身份一致；fixture / HEAD / index / Git 配置 / 正文与投影 oracle、冻结 patch 及隔离空配置均通过核对。此后只维护文档，不回写失败目录。

## 实际结果

首窗 stdout：

```text
GRAPHICS GpuSpecs { is_software_emulated: false, device_name: "AMD Ryzen 7 7700 8-Core Processor (RADV RAPHAEL_MENDOCINO)", driver_name: "radv", driver_info: "Mesa 26.2.5-arch1.1" }
STOP setup CPU draw >16 ms
```

stderr 为空。主 CSV 有 release / 硬件 renderer 身份和失败 footer，但没有数据行；spans 只有表头；没有 `STAGE` 或 summary。首窗 exit 2，后续两轮没有运行。

第一批的 None panic 未再发生；这次实际进入硬件原生窗口启动路径，并由已有 CPU draw ≤16 ms 的检查主动停止。**不是 diff 动作时延超限，也不是 diff 性能已测量完成**，不改变既有门限，不把 GPU 型号或硬件标志解释为 GPU 执行 / 屏幕呈现测量。

## 证据缺口与下一步

当前工具在没有 active Reading 的 setup 分支只保存固定停止文案，没有保存实际 draw 数值、request-layout / prepaint / paint / other 分段、逐帧尺寸或 setup 子阶段。setup 包含窗口初始绘制与首个 M-x 准备；没有 `STAGE` 只证明还未进入 diff 命令确认的计时阶段，不能进一步断言一定是冷首帧、字体或具体布局步骤变慢。

因此本批只能确认至少一个 setup CPU draw >16 ms，无法离线恢复精确耗时或归因，不能以此决定产品优化或放宽门限。此证据缺口保留为当前工具待办。

建议下一步先征得用户同意，再补最小的**单窗口 setup 诊断**：保存实际耗时和 profiler 分段，区分窗口启动与命令准备；不进入 diff 测量，不自动重试，原 16 ms 门限仍生效、超限即停止。该诊断尚未实施或授权，不自动沿用第二批三轮授权，也不预排完整性能矩阵。原有本地 headless 基线及 E3 暂缓问题保持原边界。
