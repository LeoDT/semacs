# 阶段 B：候选式 minibuffer 本地验证

本记录冻结本轮本地证据，不表示真实环境验收通过。行为正文见 [候选式读入](../emacs-fidelity.md#候选式读入阶段-b)，后续状态只在 [实施计划](../plan.md) 维护。

## 源码与环境

- 基于提交 `05d4a4e213b286306b5655a8e3a5aaa226ef472c` 的未提交阶段 B 工作树。
- 可执行源码身份：对 `src`、`tests`、`examples`、`Cargo.toml`、`Cargo.lock` 的 Git 已跟踪及未忽略新增文件，按路径排序，逐文件 `sha256sum` 后对结果再算 SHA-256：`301186f91b47d6009f0b4c71f8a09726706cecc2f2d98ca2f1711a947dad2ad0`。文档与临时产物不参与此身份。
- Linux x86_64，`rustc 1.99.1 (58cf8f9c3 2026-10-13)`；性能使用 release、GPUI TestPlatform / TestTextSystem，未访问真实桌面、原生字体或 GPU。

## 本地检查

以下检查通过：

- `cargo test --all-targets`：345 项通过。
- `cargo clippy --all-targets -- -D warnings`。
- `cargo clippy --all-targets --features native-profiler -- -D warnings`，包含修改后的原生文件 / 补全驱动编译检查，但未运行原生窗口。
- `cargo fmt --check`、`git diff --check`。

定向回归覆盖无输入候选、评分 / smart-case / Unicode 命中范围、纵向导航与输入行选择、TAB 与独立历史、RET / M-RET、目录进入与完整目录默认项、原始新路径和覆盖确认、buffer 默认项与原始名称校验，以及预编辑冻结、命令屏障、取消 / 过期异步结果隔离。旧 prompt、编辑、保存队列、关闭与焦点回归仍通过。

## Headless 性能

`perf_view` 六类约 8 MiB 样本各三轮，共 18 组；初始 960×640，改宽为 720×640，monospace 16px / 24px → bold 18px / 27px，软换行开启。按每轮数据及内置检查判定，既有预算均通过；下列为最坏值而非跨运行平均值：

| 指标 | 最坏值（ms） | 既有上限（ms） |
|---|---:|---:|
| 冷首帧 | 0.773 | 2 |
| 初始完整就绪 | 164.778 | 300 |
| 改宽完整就绪 | 164.697 | 300 |
| 改字体完整就绪 | 166.514 | 300 |
| 滚动更新 | 0.244 | 1 |
| forced frame / empty draw 均值 | 各 0.061 | 各 0.25 |
| 插入 / 后删 / 换行 / 合行 front | 0.661 | 2 |
| 插入 / 后删 / 换行 / 合行 ready | 173.810 | 300 |

`perf_m1` 共 540 行：命令链 front / ready 最坏 0.917 / 170.492 ms；1024 键未等待布局输入段 front 最坏 1.756 ms，末键恢复 ready 最坏 0.381 ms；内置预算检查通过。末键恢复不是逐键 ready 或原生持续输入。

`perf_candidates` 在 960×640、2000 个文件目录项下，各测三轮。以下均为新指标基线，没有获批时延门限，不套用正文单键预算，不表示原生呈现或字体开销：

| 动作 | action 最坏（ms） | ready 最坏（ms） |
|---|---:|---:|
| 命令过滤 | 0.147 | 0.216 |
| 命令下一候选 | 0.073 | 0.141 |
| 文件过滤 | 0.154 | 3.876 |
| 文件下一候选 | 0.134 | 0.253 |

复现入口见 [验收流程](../acceptance.md#本地检查与-headless-入口)。本轮最终原始输出为 `.pi/tmp/b-final-perf-{view,m1,candidates}.csv`，候选 fixture 为 `.pi/tmp/b-candidate-fixture-final/`；临时输出不是长期唯一证据。

原生 M2 驱动已从旧 echo 候选消息断言改为检查候选状态；其补全动作现在是“TAB 填入选中项”，输出标记 `completion_action=insert-selected`。不能把新动作与旧最长公共前缀流程直接作时延比较。

## 未测与下一步

- 未运行真实环境窗口、硬件绘制、平台 IME 或宿主性能复测；历史授权不用于本轮。
- 候选实际高度 / 高亮 / 注释截断、真实字体与字号变化、窄窗口、输入行选择标识、平台 IME 候选窗坐标仍需人工观察。
- 完整流程与建议范围见 [定向人工验证](../acceptance.md#候选式-minibuffer定向人工验证)。先获得用户同意，再准备 / 运行宿主 fish 入口；阶段 B 不据此本地记录宣告整体验收完成。
