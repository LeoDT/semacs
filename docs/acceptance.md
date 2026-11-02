# 验收流程

本文维护可执行流程，不记录历史批次的通过状态或诊断经过。预算与指标含义见 [性能预算](performance.md)，行为以 [保真度契约](emacs-fidelity.md) 为准，历史证据按需从 [文档导航](README.md#按需参考与冻结记录) 查阅。

## 分工与执行顺序

1. 助手完成受影响的代码 / 工具自检与 headless 回归，提出必要的宿主复测范围；按影响选择路径，不默认重跑完整历史矩阵。
2. 每次真实环境测试先征求用户同意。助手准备或检查 fish 入口，用户在仓库根目录的正常 fish / Wayland 宿主终端运行；隔离环境没有 `/dev/dri` 时不尝试绕过或用软件渲染冒充通过。
3. 脚本要求尚不存在的新输出目录，保留构建日志、版本 / 源码差异、环境、fixture、报告及停止原因。用户回传目录，即使失败也保留产物，不只报告退出码。
4. 助手离线核对源码 / 二进制 / 驱动 / fixture 身份、覆盖范围、硬件 / 字体 / 实际尺寸、原始数据及预算；自动结果与用户观察共同形成结论，未测项明确保留。

历史授权不延伸到下一次测试。不要覆盖旧目录、自动重试、调宽阈值或出现超限后自行调优重测；先回报现象和产物。诊断及代表性复测不外推为完整验收；按预算判定前核对适用条件，环境不同先说明可比性。

## 本地检查与 headless 入口

实现阶段收尾执行 `cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 和差异检查；若改动 profiler / 驱动，还检查相应 feature 和工具。日常小改动按影响检查，纯文档整理不机械重跑产品测试。修改人工 fixture / 验证工具时，另运行 `python3 -B -m unittest discover -s scripts -p 'test_m2_manual.py' -v`；fish 入口用 `fish --no-execute scripts/acceptance.fish` 检查语法，二者均不启动桌面窗口。

有输入 / 布局性能影响时，可在不访问桌面的 TestPlatform / TestTextSystem 上运行：

```sh
python3 scripts/perf_sample.py .pi/tmp/perf-sample.txt
cargo build --release --example perf_view --example perf_m1
RUNS=3 target/release/examples/perf_view .pi/tmp/perf-sample.txt
RUNS=3 target/release/examples/perf_m1 .pi/tmp/perf-sample.txt
```

候选更新 / 导航可另运行无新增门限的 headless 基线，fixture 目录必须尚不存在：

```sh
cargo build --release --example perf_candidates
target/release/examples/perf_candidates .pi/tmp/NEW-candidate-fixture
```

该入口在 960×640 的 TestPlatform / TestTextSystem 上各测三轮命令过滤、文件过滤和候选导航；文件 fixture 为 2000 个目录项，保留产物。它不是原生候选呈现或平台 IME 验收。

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
| `candidates` | 一个正式窗口的候选 / 路径 / buffer、字体布局及正文 / minibuffer IME 人工检查；不自动注键、不运行性能矩阵。 |
| `open-diagnostic` | 一次 mixed 文件打开的定向诊断，不继续其他样本或编辑矩阵，不作为验收。 |

自动性能模式打开测试窗口，内部使用合成 GPUI 输入；不要输入、移动 / 调整或关闭测试窗口。只有 `ime` 注入真实 OS 按键：期间不要操作键鼠，驱动注键前检查目标焦点并恢复原焦点 / 输入法。`manual` / `candidates` 不自动注键，按提示操作，不可将未观察到的项目答 yes。

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

## 候选式 minibuffer：定向人工验证

本项使用 `candidates` 模式，需要本轮真实环境测试同意。脚本准备专用可丢弃目录，保存源码差异、构建日志、二进制 SHA-256、环境 / 字体信息及观察结果；不自动注键，不测性能。不将既有 `manual` 模式的结果当作本项通过。

脚本打印的 `files` 目录为本轮全部测试文件所在位置。窗口初始打开 `source.txt`，内容为两行 `alpha second` / `next line`，clean。只操作这些文件，不使用自己的工作文件。以下各组按顺序操作；脚本在窗口退出后逐组询问，不必在终端与窗口间来回切换。建议另开本文对照。

**任何一步异常即停止，不继续后面的组、不修复 fixture 或重试。** 记录现象，必要时自行保留截图；关闭测试窗口后对失败或未观察的组答 no。若有 dirty buffer，可在退出查询明确选择不保存以结束本轮；这些只是测试文件。脚本保留失败报告和 STOP 原因，之后回传整个输出目录。

### 1. 命令、选择与取消（candidate-command）

- 初始正文按 `C-f C-f C-t C-f`，point 在 `alp` 后，mark 在 `al` 后。`M-x` 应立即展示候选、位置 / 总数及键位注释，最多 8 行，窗口不足时可更少。
- 输入 `fwd`，观察有序字符命中高亮。用方向键及 C-n / C-p 移动，输入仍为 `fwd`，边界不循环；从首候选向上可选中输入行。不要假设完整评分顺序。
- 选择 `forward-char` 后 TAB，输入变为完整命令名但未执行。C-a / DEL 不能删掉 prompt。C-g 取消后应恢复正文焦点、point、mark / active 与原滚动。
- 正文 C-g 停用 region，M-< 回到开头。再 M-x 输入 `fwd`，选择 `forward-word` 后 RET，应退出会话并把 point 移到 `alpha` 后，正文不变。
- 再 M-x 输入 `fwd`，M-RET 应提示无匹配并保留会话，不把缩写当精确命令名；C-g 取消。

### 2. 文件、目录、新名称与覆盖（candidate-files）

- C-x C-f 无输入即列出 source 所在目录，输入行被选中。输入 `su`，选中 `sub/` 后 RET 进入，输入为 `sub/`，显示子项并预选输入行；再次 RET 不应误开首文件。
- 选择 `child-document.txt` 后 TAB，应仅填入 `sub/child-document.txt`；RET 打开。C-x b 输入 `source.txt` 后 RET 返回原 buffer。
- C-x C-f 输入 `alpha`，应匹配已有的 `alpha-existing.txt`；用 M-RET 接受原始新名称，打开空的新文件 buffer。输入 **NEW-A**，C-x C-s 保存，等待 clean；C-x b 输入 `source.txt` 后 RET。
- C-x C-f 输入 `al`，从首候选向上选择输入行后 RET，打开原始新名称 `al`，不是已有候选。输入 **NEW-B**，C-x C-s 保存；再切回 source。
- source 中 M-< 后输入大写 **X**。C-x C-w 输入 `ovr`，选择 `overwrite.txt` 后 RET，应进入覆盖查询。只按 RET 不能默认同意，随后 C-g 取消，source 仍 dirty。
- 再 C-x C-w 输入 `ovr`，选择 overwrite 后 RET，再按单键 `y`。保存后身份变为 `overwrite.txt`、clean，正文为 `Xalpha second` / `next line`。原 source 文件的磁盘内容仍不变。

### 3. buffer 默认项、注释与安全确认（candidate-buffers）

- C-x b 输入 `alpha` 后 RET，进入刚创建的文件；point 到结尾 C-e，输入 **!**，使内容为 `NEW-A!`。
- 再 C-x b，候选中其他 buffer 的路径可见；C-g 取消后仍在 alpha。C-x k 应默认选中 alpha，自身候选注释为 dirty（**）及路径；RET 后出现 dirty 确认，不能按 RET 默认关闭。C-g 取消后文字 / 焦点保留；C-x C-s 保存 alpha。
- C-x b 输入 `missing-buffer` 后 M-RET，应拒绝且留在会话，不创建新 buffer。C-g 取消。
- 无输入 C-x b，核对默认项与 prompt 的默认名称相符；RET 切换到该默认 buffer。

### 4. 字体、字号、窄窗口与长名称（candidate-layout-font）

- M-x `set-font-family` RET，输入 `sans-serif` 后 RET。按实际显示观察字体变化；系统字体回退不算字体实际命中的证明。脚本另保存 fc-match 信息。
- M-x `increase-font-size` RET，重复共四次，从 16px 增至 20px。
- 正常窗口以及约 480px 宽的窄窗口中，分别检查 M-x 的候选列表。连续向下到末项、向上到输入行时，选中项始终可见、不循环；输入行、计数与列表不裁切，输入行选择标识可辨。C-g 取消。
- C-x C-f 输入 `sub/`，检查多个 long-candidate 长名称的截断和选择高亮。选择一个长名称 RET 打开，再 C-x b 输入 `overwrite.txt` 后 RET；再次 C-x b 检查长名称 buffer 的文件路径注释截断。C-g 取消。
- 保留 20px 与当前字体继续下面的 IME 检查；不要求保持窄窗口，但需记下观察时的大致窗口尺寸。

### 5. minibuffer IME（candidate-minibuffer-ime）

- C-x b 开启中文输入法，输入 ni，观察预编辑及输入法候选窗位置。编辑器的候选列表应冻结，不把未提交拼音拿去过滤；IME 自己消费的上下选择等操作应正常，不与编辑器候选混淆。
- 明确选择“你”提交，应只在 minibuffer 出现一次，提交后编辑器过滤恢复（无匹配是正常的）。关闭中文输入状态，C-g 取消；正文 / 焦点恢复且没有拼音或“你”进入正文。
- 再 C-x b 输入 ni，用输入法自己消费的 Escape 取消预编辑；应无残留拼音。关闭中文输入状态，C-g 取消读入，正文输入仍正常。

### 6. 正文 IME 与收尾（candidate-body-ime）

- C-x b 输入 `overwrite.txt` 后 RET，M-< 回到开头。开启中文输入法，ni 后明确选择“你”提交；再输入 ni，观察预编辑 / 候选窗，然后用输入法自己消费的 Escape 取消。
- 关闭中文输入状态。正文应为 **你Xalpha second** / **next line**，不重复、不残留 ni；C-x C-s 保存并等待 clean。
- 可用 M-x `reset-font-size` 恢复 16px，再 `set-font-family` 恢复 `monospace`；这些设置仅本次运行有效。
- C-x C-c 退出。全部测试 buffer 应 clean；不要额外改字。脚本询问上面六组观察以及窗口尺寸 / 字体说明，并独立核对新文件、未覆盖的原文件和所有子目录 fixture 的字节。任何未观察项目均答 no。

## 结论与证据边界

- 只有获批、实际覆盖的范围才可写为通过。原生硬件路径仍测 CPU / 平台提交，不是 GPU 执行时间或实际屏幕呈现。
- 自动成功不证明所有候选位置、字体或 F1 / F2 / F3 行为均正确；人工反馈按实际观察范围记录，不冒充助手独立重现。
- 报告核对结果按需要进入 `docs/records/`，完成状态只在计划维护；不把具体批次目录、实测和失败经过持续追加到本执行手册。
