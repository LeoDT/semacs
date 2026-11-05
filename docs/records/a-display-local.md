# 阶段 A：显示设置验收记录

固定记录本轮实现的本地、用户人工与宿主定向性能证据；行为只在 [显示设置契约](../emacs-fidelity.md#7-显示设置) 定义，阶段结论见 [阶段归档](../archive/completed-through-f1.md#阶段-a显示设置)。

## 验收结论

本地检查、六类 headless 回归、用户实际字体 / 字号及正文 / minibuffer IME 确认，与两类各三轮宿主定向性能结果共同满足阶段 A 的既定范围。未扩大为完整外观系统、所有字体 / 平台或全部历史性能矩阵的验收；保留下文未测边界，不需为本阶段追加或原样重跑测试。

## 本地检查

以下检查通过：

- `cargo test --all-targets`。
- `cargo clippy --all-targets -- -D warnings`。
- `cargo fmt --check`、`git diff --check`。

新增回归覆盖设置校验、命令取消 / 失败 / 字号边界、跨 buffer 与新建视图继承、已挂载 minibuffer 高度、显示设置不修改文档状态及历史，以及方块光标的字形簇、TAB、行尾、空行、软换行、预编辑投影和输入几何隔离。headless 使用合成字体；组合字符及 ligature 的逻辑 / 装饰断言不证明真实字体的像素效果。

## Headless 性能

当前执行环境使用 release、GPUI TestPlatform / TestTextSystem，六类约 8 MiB 样本各三轮。`perf_view` 初始尺寸 960×640，改宽到 720×640，字体由 monospace 16px / 24px 改为 bold 18px / 27px，软换行开启。

复现入口：

```sh
python3 scripts/perf_sample.py .pi/tmp/perf-sample.txt
cargo build --release --example perf_view --example perf_m1
RUNS=3 target/release/examples/perf_view .pi/tmp/perf-sample.txt
RUNS=3 target/release/examples/perf_m1 .pi/tmp/perf-sample.txt
```

`perf_view` 共 18 组，按每组原始值核对既有 headless 预算，均未超限；下表为各项最坏值，不是跨运行平均值：

| 指标 | 最坏值（ms） | 既有预算（ms） |
|---|---:|---:|
| 冷首帧 | 0.554 | 2 |
| 初始完整就绪 | 166.893 | 300 |
| 改宽完整就绪 | 167.529 | 300 |
| 改字体完整就绪 | 165.972 | 300 |
| 滚动更新 | 0.224 | 1 |
| 稳定 forced frame 均值 | 0.068 | 0.25 |
| empty draw 均值 | 0.067 | 0.25 |
| 插入 / 后删 / 换行 / 合行 front | 0.371 | 2 |
| 插入 / 后删 / 换行 / 合行 ready | 168.684 | 300 |

`perf_m1` 共 540 行：命令链 front 最坏 0.982 ms，ready 最坏 159.659 ms；每组 1024 键的未等待布局输入段 front 最坏 0.595 ms，末键恢复 ready 最坏 0.338 ms。入口内置预算检查通过；末键恢复不是逐键 ready，也不是原生长时间持续输入证据。

以上用于当前实现的 CPU 回归，不据此计算相对历史归档的性能增减。`perf_view` 的字体变更测量仍使用既有 view 设置入口，不等同于包含 `M-x` / minibuffer / 全部视图传播的完整新命令时延；命令交互已有功能回归，没有新增获批的端到端时延指标。

本轮原始临时输出为 `.pi/tmp/display-perf-view.csv` 和 `.pi/tmp/display-perf-m1.csv`；这些文件不作为长期唯一证据，指标含义与证据限制见 [性能预算](../performance.md)。

## 用户人工确认

用户已实际运行并反馈基本外观 / 交互正常，随后针对更改字体 / 字号后的正文与 minibuffer 中文 IME 再次试用，确认正常。此部分作为用户真实环境人工观察证据，不要求为流程再重复一遍。

本轮没有采集具体字体族、字号、窗口尺寸或截图，不由这份概括反馈推断所有字体、字形组合或平台均已覆盖，也不据此推断原生时延符合预算。

## 宿主定向原生性能复测

用户执行 `display-perf` fish 入口，产物 `.pi/tmp/a-display-host-perf/`。mixed、single-ascii 各三轮，共六份有序报告、426 行；按原始行独立复核 930 项数值预算条件全部通过，重算的 summary 与产物完全一致。completed / restored 为 true，stderr 全部为空，无 STOP；所有 blank_frames 为 0，六次 idle 观察均不产生 draw。

### 环境与证据身份

- 用户真实宿主，Linux 7.2.11-arch1-1，AMD Ryzen 7 7700 / RADV RAPHAEL_MENDOCINO、Mesa 26.2.5-arch1.1；六份报告均为 release / native-profiler、`is_software_emulated=false`。
- 正文实际尺寸 864×1306，改宽后 644×1306；与上轮 M2 普通原生流程一致，与早期 R1 / M1 尺寸不同，按本轮实际尺寸复核既有数值预算，不推断严格同条件的性能增减。
- monospace 匹配 Noto Sans CJK SC regular / bold；16px / 24px → bold 18px / 27px，软换行开启。fixture SHA-256：`1bbb39412c50be06cda0898f541587c1bda65d801c223d6f4d1a9269a6e5c235`。
- Git 基点 `3c090665019ddedc662b5bbb4242ddcce66e2db1`，包含未提交及未跟踪源码，不能把 HEAD 单独当作完整源码版本。`source.patch` 与核对前工作区一致；随后仅整理验收文档与任务状态。
- 二进制 SHA-256：`e90e7058f0dec0b2a4b03b00494df61934f58e3fcb4628fe6b372446a043101b`；驱动 SHA-256：`b5cdddbcdb3d583a7449231a1a61e9da2cc81d6b38c513077e95cf914d092e01`。二者及 fixture 均与核对时文件相符。

工具隔离环境无 `/dev/dri`；只读取和离线复核报告，没有重开原生窗口或用软件渲染替代本批结果。

### 最坏值

以下为六次运行各相关阶段的最大值，单位 ms；不是跨运行平均值。完整预算口径见 [性能预算](../performance.md)。

| 指标 | 最坏值 | 既有预算 |
|---|---:|---:|
| 冷打开首次 / 完整提交 | 177.647 / 344.539 | 250 / 500 |
| 字体变化首份保留或当前场景 / 完整布局提交 | 2.017 / 162.984 | 50 / 300 |
| 改宽首份保留或当前场景 / 完整布局提交 | 15.653 / 179.887 | 50 / 300 |
| 单键插入 / 后删 / 换行 / 合行同步更新 | 0.653 | 2 |
| 单键首次 / 完整提交 | 11.368 / 212.186 | 16 / 300 |
| 同回调 32 键同步 / 首次 / 完整提交 | 35.103 / 36.520 / 194.860 | 50 / 50 / 300 |
| 滚动首次提交 | 2.249 | 4 |
| 稳定强制重绘首次提交 | 0.778 | 2 |
| 单次 CPU draw | 10.719 | 16 |

### 未测边界

- 原生覆盖仅 mixed、single-ascii；未重跑其他四类、M1 长时间持续输入或 M2 文件 / 保存矩阵。
- 字体变化使用既有 view 设置入口，不是完整新 `M-x` 配置命令 / 多视图传播的端到端时延测量；命令行为由本地回归及用户试用支持。
- 仅 CPU / 平台提交，不是 GPU 执行或实际屏幕呈现；不把 idle 无 draw 写成 0 ms 帧或功耗结论。
- 人工字体 / IME 观察的范围沿用上一节，不扩大成所有字体、输入法或平台兼容承诺。

本批已执行，不覆盖或原样重跑输出目录。后续确有复测需要时，仍先征求同意并使用新目录；入口说明见 [宿主验收](../acceptance.md#原生性能与诊断)。
