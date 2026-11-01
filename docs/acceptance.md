# 验收流程

本文维护可执行流程，不记录历史批次的通过状态或诊断经过。预算与指标含义见 [性能预算](performance.md)，行为以 [保真度契约](emacs-fidelity.md) 为准，历史证据按需从 [文档导航](README.md#按需参考与冻结记录) 查阅。

## 分工与执行顺序

1. 助手完成受影响的代码 / 工具自检与 headless 回归，提出必要的宿主复测范围；按影响选择路径，不默认重跑完整历史矩阵。
2. 每次真实环境测试先征求用户同意。助手准备或检查 fish 入口，用户在仓库根目录的正常 fish / Wayland 宿主终端运行；隔离环境没有 `/dev/dri` 时不尝试绕过或用软件渲染冒充通过。
3. 脚本要求尚不存在的新输出目录，保留构建日志、版本 / 源码差异、环境、fixture、报告及停止原因。用户回传目录，即使失败也保留产物，不只报告退出码。
4. 助手离线核对源码 / 二进制 / 驱动 / fixture 身份、覆盖范围、硬件 / 字体 / 实际尺寸、原始数据及预算；自动结果与用户观察共同形成结论，未测项明确保留。

历史授权不延伸到下一次测试。不要覆盖旧目录、自动重试、调宽阈值或出现超限后自行调优重测；先回报现象和产物。诊断及代表性复测不外推为完整验收；按预算判定前核对适用条件，环境不同先说明可比性。

## 本地检查与 headless 入口

实现阶段收尾执行 `cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 和差异检查；若改动 profiler / 驱动，还检查相应 feature 和工具。日常小改动按影响检查，纯文档整理不机械重跑产品测试。

有输入 / 布局性能影响时，可在不访问桌面的 TestPlatform / TestTextSystem 上运行：

```sh
python3 scripts/perf_sample.py .pi/tmp/perf-sample.txt
cargo build --release --example perf_view --example perf_m1
RUNS=3 target/release/examples/perf_view .pi/tmp/perf-sample.txt
RUNS=3 target/release/examples/perf_m1 .pi/tmp/perf-sample.txt
```

`perf_view` 测视图布局 / 绘制与基础输入；`perf_m1` 测有界命令链与未等待布局的输入段，ready 只代表末键恢复，不是原生长时间持续输入。文件层入口为 `examples/perf_load.rs` / `scripts/perf_baseline.sh`。具体参数、样本和断言以工具为准；headless 结果不替代原生字体、GPU 或真实 IME 证据。

## 宿主 fish 入口

仅在对应测试已获同意后，将 `MODE` 和 `NEW_OUTPUT_DIRECTORY` 替换为选定模式及新目录：

```fish
fish scripts/acceptance.fish MODE NEW_OUTPUT_DIRECTORY --consent
```

脚本自动构建并准备可丢弃样本，不操作用户工作文件。当前模式以 [脚本](../scripts/acceptance.fish) 为准：

| 模式 | 适用范围 |
|---|---|
| `display-perf` | mixed / single-ascii 各三轮的原生基础性能代表性复测，包含字体变化与绘制；不跑 M1 持续输入或 M2 文件矩阵。 |
| `perf` | 文件 / buffer M2 六类矩阵、普通原生六类矩阵及 M1 两类代表性持续输入；开销较大，不作为每次改动的默认检查。 |
| `ime` | mixed / single-ascii 各三轮真实正文 IME；需要 Hyprland、wtype、fcitx5-remote，grim 可选。完整 minibuffer IME 仍需人工观察。 |
| `manual` | 两个正式窗口的键盘文件 / IME / OS 关闭流程，按下文操作，脚本核对观察回答与磁盘字节。 |
| `open-diagnostic` | 一次 mixed 文件打开的定向诊断，不继续其他样本或编辑矩阵，不作为验收。 |

自动性能模式打开测试窗口，内部使用合成 GPUI 输入；不要输入、移动 / 调整或关闭测试窗口。只有 `ime` 注入真实 OS 按键：期间不要操作键鼠，驱动注键前检查目标焦点并恢复原焦点 / 输入法。`manual` 不自动注键，按提示操作，不可将未观察到的项目答 yes。

原生性能验收要求硬件渲染；软件诊断不能套用硬件验收结论。fish 入口不提供软件降级；底层驱动的 `--software` 仅供另行授权的诊断。普通原生 / IME 模式在每份窗口报告完成后判定，M1 / M2 还会在测量过程中检查既有预算；失败停止后续运行。模式与参数含义见 [驱动](../scripts/perf_native.py)，不以本页重述完整动作矩阵。

## 显示设置：定向原生性能复测

`display-perf` 覆盖真实字体布局、绘制和输入的代表性路径，不表示其余四类原生样本或长时间持续输入已复测。字体阶段沿用 monospace 16px / 24px → bold 18px / 27px 的既有 view 入口，不是完整新 `M-x` 配置命令及所有视图传播的端到端时延测量。

<a id="首轮超限后的单次诊断"></a>

## 单次打开诊断

`open-diagnostic` 只运行 scratch 准备和一次正式文件打开；既有预算仍适用，`acceptance=false`，诊断成功也不自动继续矩阵。保留 CSV、逐帧 / 慢 view / 子调用报告与 diagnostic.json，即使失败也交回分析。报告字段与关联校验以 [诊断实现](../examples/support/native_diagnostic.rs) 及 [驱动](../scripts/perf_native.py) 为准；历史超限、定位与修复经过已冻结于 [验收记录](records/m2-m3-local.md#首轮宿主停止与诊断交付)。

## 人工：第一个窗口

脚本打印本轮文件目录，下文的路径均指该目录中的可丢弃文件。不要在自己的工作文件上操作。遇到任一步异常，记录现象并答 no，停止，不通过修改期望字节掩盖问题。

1. 初始打开 `alpha.txt`，显示两行 alpha / second，初始 clean。
2. `C-x C-f` 输入 **完整 beta.txt 路径**并 RET。正文开头输入大写 `Y`；`M-x` 输入 `save-buffer` 并 RET。应显示 Ybeta，保存后 clean。
   - 可在输入命令名前用 C-a / DEL 检查 prompt 不会被删除，随后正常输入命令名。
3. `C-x b` 输入 `alpha.txt` 并 RET，正文开头输入大写 `X`。
4. 再 `C-x b`，在 minibuffer 开启中文输入法，输入 ni，观察候选位置正常，选择“你”；确认文字只进入 minibuffer，随后 `C-g`。正文仍为 Xalpha / second，point 应恢复到 X 后，焦点在正文。关闭中文输入状态。
5. 正文开启中文输入法，ni 后选择“你”；再输入 ni，观察预编辑 / 候选，然后用输入法消费的 Escape 取消预编辑。关闭中文输入状态。正文最终应为 **X你alpha / second**，无残留 ni、不重复提交。
   - 如果候选首项不是“你”，明确选择“你”，不要把不同候选结果记为程序缺陷。
6. `C-x C-s` 保存 alpha，等待 clean；`C-x C-w` 输入 **完整 overwrite.txt 路径**并 RET。应出现覆盖查询；只按 `y`，不加 RET，即可保存。文件身份变为 overwrite.txt，焦点恢复正文；用 C-f / C-b 检查运动即可，不再插入额外正文。
7. `C-x b` 切到 beta.txt，确认仍为 Ybeta；`C-x k RET` 关闭该 clean buffer。回到 overwrite.txt 后 `M-x C-g` 应取消并回显 Quit，随后 `C-x C-c` 正常退出。

在终端分别确认键盘文件闭环、正文 IME、minibuffer IME。脚本最后检查 alpha / overwrite 的 BOM、CRLF、无末尾换行及精确文字，beta 保持 LF 与末尾换行。UI 状态和候选位置仍以你的观察为证。

## 人工：第二个窗口

1. 初始打开 close.txt，在开头输入大写 `Z`；`M-x` 输入未完成的命令名 `for`。
2. 使用平时合成器的 **关闭窗口快捷键 / 标题栏关闭按钮**，不要用 `C-x C-c` 代替 OS 关闭。当前实现应取消读命令会话并出现保存询问，而不是立即丢数据。
3. 按 RET，不能默认同意或关闭；再 `C-g` 取消。窗口必须保留，Zclose 仍在，焦点恢复正文。用 C-f / C-b 检查即可。
4. 再触发 OS 关闭，在保存询问按单键 `n`。应继续询问是否放弃仍修改的 buffer；输入完整 `no RET`，取消退出，文字仍保留。
5. `C-x C-s`，等待 clean，然后再触发 OS 关闭，应直接退出。脚本独立检查 close.txt 为 `Zclose\n`。

若平台只能走已批准的窗口级确认例外，记录实际流程、安全默认取消与焦点情况，先回报，不直接把不符上述预期的步骤勾成通过。

## 结论与证据边界

- 只有获批、实际覆盖的范围才可写为通过。原生硬件路径仍测 CPU / 平台提交，不是 GPU 执行时间或实际屏幕呈现。
- 自动成功不证明所有候选位置、字体或 F1 / F2 / F3 行为均正确；人工反馈按实际观察范围记录，不冒充助手独立重现。
- 报告核对结果按需要进入 `docs/records/`，完成状态只在计划维护；不把具体批次目录、实测和失败经过持续追加到本执行手册。
