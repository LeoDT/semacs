# M1 性能验收记录

固定记录 2026-11-01 的验收结论、环境和实测；预算与测量入口见 [性能预算](../performance.md)。

## 验收结论

M1 本阶段验收通过。用户明确确认持续输入的适用口径：每键同步更新 / 首次目标提交须满足既有预算；被后续输入取代的旧版本不要求继续完成完整布局，停止输入后的最终有效版本须在 300 ms 内完整就绪。实际观测到的 ready 仍按对应预算判定，不调宽数值阈值。

- 当前代码的 headless 六类各三轮回归、用户宿主 RADV 原生 M1 六类各三轮均通过。
- 全部 21600 键均观察到首次目标提交；14514 个 ready 被观测到，7086 个目标在就绪前被后续输入取代，后者保留 ready 未测，不填 0 或计作逐键完成。
- 所有 18 次原生运行的末键均完整就绪。该结论不证明 21600 个中间版本都在 300 ms 内完成完整布局，也不由工具的 completed=true 单独推导。
- 用户要求适度测试、超预算先汇报。未因预算超限修改产品代码或阈值、反复调参重测；正式 Root 路由缺陷在汇报并获同意后修复，最终测量使用修复版本。

## 固定环境

- Linux 7.2.11-arch1-1，release；六类约 8 MiB 正文，软换行开启。样本沿用 R1 配方。
- fixture SHA-256：`1bbb39412c50be06cda0898f541587c1bda65d801c223d6f4d1a9269a6e5c235`，测试后未变化。
- 宿主：Hyprland 0.56.2，RADV RAPHAEL_MENDOCINO / Mesa 26.2.5，`is_software_emulated=false`；请求 monospace，fontconfig 匹配 Noto Sans CJK SC。
- 原生 M1 使用 native-profiler，monospace 16px / 24px，实际正文 838×1306 px；合成 GPUI 按键，不是 OS 键盘或 IME 端到端输入。
- Headless 使用默认 feature、TestPlatform / TestTextSystem。`perf_view` 保留历史 AppShell 边界；`perf_m1` 及正式输入回归使用与产品一致的 Base Root。
- 工具隔离环境无 `/dev/dri`，没有绕过隔离；硬件矩阵由用户在正常宿主终端执行。软件诊断显式使用 llvmpipe / Mesa 26.2.5，不能套用宿主硬件预算。

## 原生 M1 完整矩阵

六类各三轮、共 18 个独立窗口，每轮为 29 步命令链及 60 秒 / 50 ms、1200 键持续输入。复用两次首轮测量并补其余 16 轮，没有重跑已有轮次。所有报告的环境、字体、尺寸、fixture 摘要和参数一致，completed / restored 为 true，stderr 为空。

22140 条有序阶段数据、522 步命令及 21600 键完整；逐行独立核对 103614 项可观测预算条件，无超限、无空白正文帧，不以平均值掩盖回退。以下为每类三轮的最大值，末键 ready 列为三轮范围，单位 ms；未测 ready 不参与最大值计算。

| 正文 | 命令链同步 / 首次 / ready | 持续输入同步 / 首次 | 末键 ready 范围 | 持续段已测 ready / 未测 ready |
|---|---:|---:|---:|---:|
| mixed | 0.484 / 11.807 / 11.807 | 0.178 / 11.737 | 0.698–0.863 | 3600 / 0 |
| short-ascii | 0.638 / 1.727 / 1.727 | 0.205 / 1.574 | 1.245–1.287 | 3600 / 0 |
| short-unicode | 0.521 / 2.548 / 2.548 | 0.211 / 2.930 | 1.556–1.686 | 3600 / 0 |
| long-ascii | 0.795 / 2.124 / 14.379 | 0.169 / 2.378 | 4.596–5.155 | 3600 / 0 |
| single-ascii | 0.790 / 2.060 / 214.116 | 0.277 / 2.140 | 171.597–188.447 | 57 / 3543 |
| single-unicode | 0.690 / 2.468 / 147.420 | 0.289 / 2.935 | 122.328–122.573 | 57 / 3543 |

冷打开首次 / 完整提交最大 165.965 / 363.593 ms，满足 250 / 500 ms；命令链同步 / 首次 / ready 最大 0.795 / 11.807 / 214.116 ms；持续段同步 / 首次最大 0.289 / 11.737 ms，末键恢复最高 188.447 ms，满足 2 / 16 / 300 ms 的对应预算。已记录单次 CPU draw 最大 11.442 ms，低于 16 ms。

两类单条超长行每轮仅观测到 19 个完整 ready，其余 1181 个目标在完成前被后续键取代。不能由其他短行样本的完整 ready 覆盖推断超长行能逐键追上 50 ms 节奏。调度迟到最大 17.170 ms 是计划注入到动作开始的差值，不是 action-to-submit，不适用首次提交的 16 ms 门限。

## 修复后当前代码的 headless 收尾

均为 release 默认 feature、六类各三轮。`perf_view` 初始 960×640，改宽 720×640，monospace 16px / 24px → bold 18px / 27px；`perf_m1` 固定初始尺寸 / 字体并包含 Base Root。全部文本、point、非空正文及完整索引断言通过。

| 项目 | 最大值 ms |
|---|---:|
| perf_view 冷首帧 | 0.680 |
| 初始 / 改宽 / 字体完整就绪 | 164.245 / 165.337 / 163.350 |
| 滚动更新 | 0.218 |
| forced frame / empty draw（各次均值） | 0.063 / 0.061 |
| 四类输入 front / ready | 0.357 / 167.579 |
| perf_m1 同步 / front / ready | 0.657 / 0.758 / 166.344 |

全部满足 headless 预算。`perf_m1` 的有界连续前台段不在键间主动 drain 测试后台任务，其 ready 只代表末键恢复，不代替原生长时间持续输入。此前无 Root 的开发阶段 CPU 结果未作为修复后代码的性能证据。

## 补充诊断及其适用范围

### 用户宿主普通基线：mixed 单轮

RADV、release / profiler；正文初始 838×1306 px，改宽后 624×1306 px。71 条阶段数据完整、227 项预算检查无超限，completed / restored 为 true，stderr 为空，fixture 未变化。单位 ms，重复阶段取最大值：

| 项目 | 实测 |
|---|---:|
| 冷打开首次 / 完整提交 | 157.312 / 355.844 |
| 单次 CPU draw | 10.657 |
| 滚动 / 稳定强制重绘首次提交 | 2.110 / 0.784 |
| 改宽首份保留场景 / 完整提交 | 14.214 / 169.759 |
| 字体首份保留场景 / 完整提交 | 1.949 / 155.005 |
| 四类单次输入同步 / 首次 / 完整提交 | 0.558 / 10.940 / 10.940 |
| 同回调 32 次输入同步 / 首次 / 完整提交 | 30.581 / 31.839 / 31.839 |

所有阶段 blank_frames=0，idle 观察阶段 frames=0。改宽 / 字体的 50 ms 预算比较首份场景提交，而非要求新布局的首次目标提交。该单轮只是基线，不代替 M1 的完整矩阵。

### 真实 Wayland / IME 与短时软件功能诊断

修复 Root 路由前的 mixed 单轮 llvmpipe / wtype / fcitx5 Pinyin 诊断，完成两次 ni 预编辑、取消、滚动 / 改宽 / 字体 / 点击后的预编辑与提交“你”。文本、marked、modified、完整索引和正文断言通过，两张候选截图可见“你”；焦点 / 输入法恢复，completed / restored 为 true。

arm-to-submit 首次目标状态为 21.376–48.633 ms，包含外部驱动 / IPC / IME。软件滚动首次提交 5.802 ms 虽高于硬件门限数值 4 ms，但渲染器不适用该门限；未据此宣布硬件通过、判定硬件回退或调参重测。

Root 修复后的 single-ascii 单次 1 秒 / 50 ms 软件自检，29 步命令链及 20 键持续段完成；19 个 ready 被取代，末键完整就绪，completed / restored 为 true、stderr 为空。它只验证短时功能，不作为硬件预算或真实 IME 的复测证据。

## 检查与产物

- 默认 244 项 / native-profiler 257 项 Rust 测试、16 项 Python 驱动测试通过；两组 clippy `-D warnings`、fmt / diff 检查通过。提交前复核通过，未追加性能测试。
- 原生完整矩阵来源：`.pi/tmp/m1-native-20261101-113802/`、`.pi/tmp/m1-native-long-20261101-114710/`、`.pi/tmp/m1-native-rest-20261101-115316/{repeats,remaining}/`。
- 合并核对：`.pi/tmp/m1-perf-acceptance/native-m1-matrix-check.json`，逐份记录来源、环境、每类覆盖、可观测预算及未测 ready。
- 当前代码 CPU 收尾：同目录 `final-perf_view.csv`、`final-perf_m1.csv`、对应 stderr 与 `final-headless-check.json`。
- 其他诊断：`.pi/tmp/m1-hardware-20261101-111303/`、`.pi/tmp/m1-perf-acceptance/wayland-ime-software/`、`.pi/tmp/m1-native-entry-root-fixed-smoke/`。
- 提交检查日志：`.pi/tmp/m1-commit-tests*.log`、`m1-commit-clippy*.log`。临时数据不纳入 git；复现入口由正式 examples / scripts 维护。

## 未测边界与后续范围

- 7086 个被取代 ready 未测；按用户确认口径不要求过期目标继续完成布局，不外推逐键完整布局时延。
- 合成 GPUI dispatch 不证明真实 OS 键盘 / IME 链路的延迟。本轮没有新的硬件 IME 性能矩阵；R1 的硬件 IME 记录仍只是 R1 历史证据。
- GPU 执行、实际屏幕呈现和纯实体按键端到端时延未测，沿用已批准的非 M1 硬门禁边界，不由 CPU 提交或截图推算。
- 完整 minibuffer 读入 / 补全、文件保存及多 buffer 管理由 M2 实施；M3 再做范围内保真度 / IME / 性能汇总，不提前宣称 v1 完成。
