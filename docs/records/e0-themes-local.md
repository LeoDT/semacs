# E0 基础主题本地验证

## 身份与环境

- 基线：`9bff7046a03caea226621b3039b21ec69faabd73` 加本轮未提交 E0 修改。
- 源码身份：对 `git ls-files --cached --others --exclude-standard` 在 `Cargo.toml`、`Cargo.lock`、`src/`、`tests/`、`scripts/`、`examples/`、`types/` 下列出的文件去重、按路径排序，生成每行 `SHA256  路径\n` 的清单；154 个文件，清单 SHA-256 为 `d59f5ddfb0332c723ef34d8e746277b0de4021a77ca413b962917f16df262ff3`。不包含 docs 和本地临时产物。
- 正式 release `semacs` SHA-256：`01b73533e49ac8060b72b3f4afa86e458bfe428a4d41d6dbc5383fbf638bd316`。
- Linux `7.2.11-arch1-1`、x86_64；rustc `1.99.1 (58cf8f9c3 2026-10-13)`。
- GPUI TestPlatform / TestTextSystem；未访问真实桌面。性能构建为 release，软换行开启。

## 本地结果

- `cargo test --all-targets`：453 项通过。
- 默认及 `native-profiler` 的 all-targets Clippy（`-D warnings`）、`cargo fmt --check`、差异检查通过。
- 现有 Python tests 50 项、manual fixture 工具 14 项通过；fish 入口语法检查、示例 JS 与类型声明的 TypeScript 检查通过。正式 release 二进制与两个 headless 性能入口构建成功。
- 集中 reviewer 一次，发现换色时超长字形行的缓存被清空，首帧可能丢失正文 / 输入坐标。修复后颜色只在 paint 装饰中解析，不参与 shaping 缓存或首屏准备身份；主代理回归确认不等待后台任务的换色首帧仍有行、可用输入坐标且复用原字形缓存，后台首屏结果也能跨换色复用。未追加审查。

### 既有 profiler 工具失败与证据边界

完整 `cargo test --all-targets --features native-profiler` 在旧 M2 workload 的 step 30 Kill 报 `lifecycle did not return to one scratch buffer`。在独立归档源码目录对未修改基线 `9bff704` 运行同一测试也失败；未因此改变生命周期、放宽断言或记作 feature 全套通过。

审查确认该断言也在实际原生 M2 驱动中，影响 `scripts/acceptance.fish perf`，并非仅测试代码。失败保持 fail-closed，后续处理见 [计划](../plan.md#已知问题与未排期事项)。

审查修复后的代码以精确 `--skip m2::tests::native_workload_runs_through_the_production_shell_headlessly` 运行其余 feature 测试：481 项通过、排除 1 项。此结果不替代完整 feature 通过结论，也没有运行原生 perf 批次。

## Headless 回归

修复后重新构建并顺序执行 `perf_view` 与 `perf_m1`，六类约 8 MiB 样本各三轮，不并行跑性能进程。混合样本为 8,388,648 字节。视图入口初始 960×640、改宽 720×640，monospace 16px / 24px 转 bold 18px / 27px；M1 沿用入口的 960×640、16px / 24px。

`perf_view` 共 18 行结果，各项逐次均在已批准 headless 门限内。以下为本批最大值，不用平均值掩盖波动：

| 项目 | 最大值 ms |
|---|---:|
| 冷首帧 | 0.589 |
| 初始完整索引 | 151.476 |
| 滚动 | 0.221 |
| 改宽完整索引 | 151.846 |
| 改字体完整索引 | 152.251 |
| forced frame / empty draw 均值 | 0.062 / 0.061 |
| 插入 / 后删 / 换行 / 合行 front | 0.402 / 0.420 / 0.363 / 0.358 |
| 插入 / 后删 / 换行 / 合行 ready | 163.742 / 163.530 / 162.806 / 165.300 |

`perf_m1` 共 540 行，最大同步动作 0.645 ms、front 0.830 ms、实测 ready 158.484 ms；chain 的逐动作与 unsettled-input 的最终恢复均在对应 headless 门限内。未完成的中间 ready 留空，不能解释为逐键完成或原生持续输入通过。

这是既有绘制 / 输入路径的 CPU 回归，不是主题切换新时延指标、原生字体、真实 IME、GPU 或屏幕呈现证明。没有新增或放宽性能预算。

## 留存与下一步

本地原始 CSV、检查输出、源码清单与摘要保留于 `.pi/tmp/e0-final-*`、`.pi/tmp/e0-all.log` 和 `.pi/tmp/e0-source-*`；原先 feature / 未修改基线失败日志分别为 `.pi/tmp/e0-profiler-tests.log` 与 `.pi/tmp/e0-base-profiler-test.log`。这些本地路径不是长期契约或待办的唯一存放处。

E0 实现和本地验证完成，尚待用户同意后观察真实窗口效果。入口见 [E0 验收流程](../acceptance.md#宿主-fish-入口)，使用独立配置及可丢弃文件，不修改日常配置。未宣布阶段最终验收完成，未启动 E1。
