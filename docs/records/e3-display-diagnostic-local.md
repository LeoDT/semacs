# E3 单窗口完整显示诊断：本地工具验证

## 范围与身份

接续 [修复后的首屏失败](e3-native-perf-host-02.md)。用户认为单次首屏仅超约 10 ms 不足以决定继续优化，已批准先查看一个 mixed 窗口的完整布局、滚动和输入数据。工具增加显式非验收模式，未修改产品换行算法、原预算或旧失败目录。有效分工与安全上限见 [完整显示诊断流程](../acceptance.md#单窗口完整显示诊断)。

HEAD 仍为 `138995b31a615833e04a729e8d619c829b9667c4`，含整批未提交 E3 及工具改动。最终源码 SHA-256：

- `examples/perf_native.rs`：`3d424ed76de1e653b847b6bef247e4a0f59b722c186e478f51b54c39b13c24da`
- `scripts/perf_native.py`：`241d67b5ca3460c79e52defabc04e4154f0434512416ba69d7ff06abd2f25b61`
- release / native-profiler 实际 probe：`c622768270787ec8afc82be2cdec498c34c755a593e368fd438fcc6d90654d10`

原始检查日志及完整摘要在 `.pi/tmp/e3-display-diagnostic/`。用户真实终端的单窗口包装入口为 `.pi/tmp/e3-display-diagnostic-once.fish`，尚未运行原生批次；永久 CLI 入口为 `perf_native.py --display-diagnostic --sample mixed --runs 1 --consent`。

## 检查结果

- 默认全目标 **533 项**、native-profiler 全目标 **582 项**，通过且无跳过。
- 两套 Clippy `-D warnings`、**81 项 Python 测试**、Rust 格式、fish 语法与差异检查通过；release probe 构建通过。
- TestPlatform 使用生产 `Run`、实际 view / 命令与合成提交边界，故意让 open 超限后继续走完 **71 个动作**，覆盖滚动、改宽、字体、点击、插入 / 删除 / 换行 / 合行、32 键与 idle。逐行核对 CSV 即时落盘，最终正文恢复；报告完整但 budget_passed=false、acceptance=false。
- 生产 Run 的正文不匹配、2 秒观察超时及原有普通验收超限停止回归通过。硬停止保存部分失败行，不消费下一动作；未知 ready 仍留空。
- Python 覆盖完整 / 超限报告、顺序与身份、诊断不得混作验收、软件类别、参数范围、单窗口隔离配置与非验收元数据、父进程阶段 watchdog。独立假子进程测试确认超过阶段上限会被终止并保留部分 CSV / 日志，不等待 20 秒 sleep。

## 边界

普通验收仍在性能超限时停止；只有显式诊断可记录超限并继续，收集完成不表示预算通过。数据、预算结论与采集 / 安全停止分开。阶段 2 秒、启动前 5 秒是诊断资源保护，不是新验收预算。

本轮没有启动原生窗口、读取系统字体、用户配置或做新的性能矩阵；没有追加 reviewer。没有重跑产品 headless 计时，因产品算法未变，本轮验证的是 probe / 驱动采集与停止逻辑；上述 TestPlatform 与假子进程不冒充原生时延或 GPU 结果。下一步由用户真实终端运行新的单 mixed 诊断目录，再依据完整数据决定优化优先级；不自动扩展三轮、Markdown 或六窗口矩阵。
