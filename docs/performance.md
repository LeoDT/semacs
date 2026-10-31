# 性能预算与测量边界

这里维护已批准预算、指标含义与必要入口，不维护样本配方、实现阈值或脚本断言清单。
预算适用的固定环境、尺寸与历史数据见 [R1 验收归档](records/r1.md)。

## 适用原则

- 既定预算不通过调宽阈值掩盖回退；环境、样本或构建条件变化时先明确可比性，不直接套用或改写旧结论。
- 六类约 8 MiB 正文、软换行开启，区分冷首帧与最终精确布局；不能以截断正文、关换行、漏绘或额外热重绘换取数字。
- 各项至少三次，记录波动；原生预算每次运行分别判定，不以平均值掩盖超限。
- headless 仅用于 CPU 回归，不代表原生字体、GPU 或真实 IME；软件渲染自检与宿主硬件验收分开。

## 指标边界

所有时间为墙钟耗时；CPU 侧可能包含驱动等待，不是 CPU 周期或 GPU 执行时长。

- headless `first` 从新窗口创建前到首次正文 paint，`ready` 到完整精确索引及可见 shaping 就绪；输入 `front` 包含按键分发、编辑与首次布局 / paint，输入 `ready` 使用同一起点。
- 原生 `action_cpu_ms` 是同步动作分发 / 更新，不含后续 draw；draw / submit 是 CPU 调用成本。
- `first_frame_submit_ms` 是首份场景提交，可能仍是旧布局的保留正文；`first_submit_ms` 要求目标文本 / marked 状态及兼容布局，`ready_submit_ms` 还要求完整索引与可见 shaping 就绪。
- 自动动作采用 action-to-submit；真实 IME 采用 arm-to-submit，包含 IPC、wtype、合成器、fcitx5 及固定按键间隔，不是纯实体按键或 IME 回调延迟。
- `observed_elapsed_ms` 包含驱动后续观察及计时外断言，不是 ready 或输入延迟。
- headless forced frame 是无文本变化的强制重绘均值，empty draw 是未主动 invalidation 的 draw 均值；原生 idle 只观察是否实际 draw，不能据此报告 0 ms 帧或空闲功耗。
- 这些入口没有 GPU timestamp 或实际呈现反馈；CPU 提交、候选截图和 on_next_frame 均不证明屏幕呈现完成，也不额外加一个刷新周期估算呈现时间。

## 已确认的 headless 预算

适用于归档中的 TestPlatform / TestTextSystem、release、六类样本及固定尺寸 / 字体。

| 项目 | 上限 |
|---|---|
| 冷首帧 | ≤ 2 ms |
| 初始 / 改宽 / 字体完整索引就绪 | 各 ≤ 300 ms |
| 滚动更新 | ≤ 1 ms |
| 稳定 forced frame / empty draw 均值 | 各 ≤ 0.25 ms |
| 插入 / 后删 / 换行 / 合行 | front ≤ 2 ms；ready ≤ 300 ms |

## 已确认的原生 CPU / 提交流程预算

适用于归档中的宿主硬件、release / profiler、六类样本与实际尺寸。

| 项目 | 上限 |
|---|---|
| 冷打开 | 首次正文提交 ≤ 250 ms；完整布局提交 ≤ 500 ms |
| 单次 CPU draw | ≤ 16 ms，不代表 GPU 帧时间 |
| 插入 / 后删 / 换行 / 合行 | 同步更新 ≤ 2 ms；首次提交 ≤ 16 ms；完整就绪提交 ≤ 300 ms |
| 滚动 / 稳定强制重绘 | 每次首次提交分别 ≤ 4 ms / ≤ 2 ms |
| 改宽 / 字体 | 第一份保留或当前场景提交 ≤ 50 ms；完整新布局提交 ≤ 300 ms；不允许空白正文帧 |
| 同回调 32 次插入 / 删除 | 整批同步更新和首次提交各 ≤ 50 ms；完整就绪提交 ≤ 300 ms |
| 真实 IME 受控流程 | arm 首次目标状态提交 ≤ 75 ms；完整就绪提交 ≤ 300 ms |
| 稳定观察阶段 | 30 个无主动 invalidation 的 frame-demand tick 不产生 draw |

同回调 32 键不是长时间持续输入；完整命令链和持续输入的阶段补测见 [实施计划](plan.md)。
GPU 执行 / 实际呈现可在出现瓶颈或需要端到端延迟承诺时另定方案，不预先扩大为 M1 硬门禁。

## 测量入口

样本定义见 `scripts/perf_sample.py` 与 `examples/support/samples.rs`，参数和校验以程序为准。

```sh
python3 scripts/perf_sample.py .pi/tmp/perf-sample.txt
cargo build --release --example perf_view
RUNS=3 target/release/examples/perf_view .pi/tmp/perf-sample.txt
```

文件层独立入口为 `examples/perf_load.rs` / `scripts/perf_baseline.sh`。

以下命令仅在另行获得真实桌面测试同意后，在正常宿主终端运行；输入使用可丢弃样本，输出目录必须尚不存在：

```sh
cargo build --release --example perf_native --features native-profiler
python3 -B scripts/perf_native.py .pi/tmp/perf-sample.txt .pi/tmp/native-report --consent
python3 -B scripts/perf_native.py .pi/tmp/perf-sample.txt .pi/tmp/native-ime-report --consent --ime
```

硬件验收不强制软件渲染；`--software` 仅用于诊断，不代表硬件通过。原生功能诊断入口为 `examples/native_input.rs`，不能将其状态日志时间戳当作性能数据。
