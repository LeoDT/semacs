# 验收流程

本文维护验证范围的选择、本地 / 宿主分工、可执行入口与安全规则。行为见 [保真度契约](emacs-fidelity.md)，预算与指标见 [性能预算](performance.md)，当前工作从 [计划](plan.md) 查阅，历史结论见 [R0–F1 归档](archive/completed-through-f1.md)、[F2 归档](archive/f2-status.md)、[F3 归档](archive/f3-window-commit.md)、[F4 归档](archive/f4-status-hunks.md)、[F5 归档](archive/f5-history-commit.md)、[F5 后插入项归档](archive/post-f5-insertions.md)和 [冻结记录索引](records/README.md)，不在本文逐阶段累积覆盖清单或批次结论。

## 宿主验证的适用条件

宿主验证是在实际目标运行环境中确认行为或指标；本项目通常指真实 Wayland、平台字体 / 输入法及硬件渲染。它不等于性能测试，也不把使用临时磁盘文件或普通子进程的自动测试都算作宿主验证。

**只有本次受影响的行为或指标依赖真实环境，且现有自动化 / headless 证据不能可靠覆盖时，才提出最小必要的宿主验证。** 不按阶段、文件名或“改到了 UI 代码”机械触发。

| 改动 / 待确认内容 | 默认选择 |
|---|---|
| 编辑语义、history、解析 / 版本隔离、配置校验、文件字节往返、工具断言 | 本地自动测试；有 CPU / 布局影响时补 headless 回归，不因此自动安排宿主测试 |
| 真实字体 / fallback、颜色与光标可读性、窗口布局、焦点、平台键盘 / 关闭行为、IME 组合及候选位置 | 对现有测试无法覆盖的受影响部分，提出定向宿主功能验证 |
| 实际平台的原生输入 / CPU 提交性能、硬件渲染或实际呈现 | 先核对测量能力和预算，再选对应宿主入口；当前 CPU 提交工具不提供 GPU 执行 / 屏幕呈现证明 |
| 其他只有实际系统才能复现的集成问题 | 先说明隔离测试的缺口、目标与最小操作，再征求同意，不泛化成完整矩阵 |

提出宿主测试时说明“本次改动 → 尚缺的证据 → 所选模式 / 最小范围”。已有可靠覆盖且本次不影响平台行为时不追加宿主测试；已接受的未测边界或历史非阻塞观察不自动变成补测任务。需要的证据尚未取得时如实保留未测，不写成通过。

## 分工与执行顺序

1. 助手先完成受影响的本地检查与 headless 回归；按上节判断是否需要宿主验证，不默认重跑完整历史矩阵。
2. 每批真实环境测试都须先获得用户对具体范围的同意，提前说明窗口、OS 注键、输入法状态变动及截图范围。历史授权不延续到下一批，脚本的 `--consent` 参数不能代替实际同意。
3. 助手准备或检查入口，用户在仓库根目录的正常 fish / Wayland 宿主终端运行。没有 `/dev/dri` 时不绕过检查或用软件渲染冒充硬件通过。
4. 使用尚不存在的新输出目录和可丢弃 fixture，不操作日常工作文件。人工模式只给测试子进程设置独立 `XDG_CONFIG_HOME`，不读写日常配置；`manual` / `candidates` 的空配置也参与字节核对。
5. 默认验收遇超限、断言失败、异常或未观察项立即停止后续测试，保留日志和产物，不修复 fixture、自动重试、调宽阈值或自行调优重测；先回报。另行获准的完整显示 / diff 诊断只改变性能超限的停止策略：记录超限后继续该批后续动作及获准窗口，始终标记非验收；正文 / 投影 / 几何 / 身份错误、异常和安全超时仍硬停止。人工模式按提示结束测试窗口，对失败 / 未观察项答 no，不把正常退出当成通过。
6. 用户回传整个目录，失败也保留。助手离线核对源码 / 二进制 / 驱动 / fixture 身份、覆盖范围、环境 / 字体 / 实际尺寸、原始数据和适用预算；只对实际覆盖内容下结论，人工观察不冒充助手独立复现。

输出保留构建日志、版本 / 源码差异、环境、fixture、报告及停止原因，不覆盖旧目录。结论按需进入 `docs/records/`，完成状态只在计划维护；未测项明确保留，代表性复测不外推为完整验收。环境、样本或构建不同，先说明可比性再判预算。

## 本地检查与 headless 入口

日常小改动按影响检查，纯文档不机械重跑产品测试。实现阶段收尾的基本检查为 `cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 和 `git diff --check`。额外检查按改动选择：

| 影响范围 | 离线检查 |
|---|---|
| profiler / 性能驱动 | `cargo test --all-targets --features native-profiler`、对应 feature 的 Clippy，以及 `python3 -B -m unittest discover -s tests -p 'test_*.py'` |
| 人工 fixture / 字节核对工具 | `python3 -B -m unittest discover -s scripts -p 'test_m2_manual.py' -v`；Magit profile 另跑 `python3 -B -m unittest discover -s scripts -p 'test_magit_fixture.py' -v`；F2 单窗口入口另跑 `python3 -B -m unittest discover -s scripts -p 'test_magit_status_manual.py' -v` |
| 脚本配置冒烟驱动 | `python3 -B -m unittest discover -s scripts -p 'test_scripting_manual.py' -v` |
| split IME 驱动 | `python3 -B -m unittest discover -s scripts -p 'test_split_ime.py' -v`，不访问桌面 |
| F3 单窗口 commit 入口 / fixture | `python3 -B -m unittest discover -s scripts -p 'test_magit_commit_manual.py' -v`；与 F2 / F1 共用入口变更时补相应工具回归，不启动桌面 |
| fish 入口 | `fish --no-execute scripts/acceptance.fish` |
| JS 示例 / 类型声明 | 本地已有 tsc 时运行 `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts`；它不是运行依赖 |

输入 / 布局 CPU 回归可使用 TestPlatform / TestTextSystem，不启动桌面：

```sh
python3 -B scripts/perf_sample.py .pi/tmp/NEW-sample.txt
cargo build --release --example perf_view --example perf_m1
RUNS=3 target/release/examples/perf_view .pi/tmp/NEW-sample.txt
# 定向样本可设置 SHAPES=mixed,single-ascii；未知 / 空 / 重复名称拒绝。
# perf_view 逐阶段检查既有预算，首次超限立即退出，不继续后续运行。
RUNS=3 target/release/examples/perf_m1 .pi/tmp/NEW-sample.txt
```

其他入口按影响选择，先用 `cargo build --release --example NAME` 构建对应 example；不是要求每轮全跑：

| 入口 | 范围与解释 |
|---|---|
| `python3 -B scripts/perf_status.py .pi/tmp/NEW-status-perf` | release `perf_status`，64 / 4096 条隔离清单各三轮；原生 status / 刷新、随包 JS 整文件 `s` / `u`、内嵌 diff 展开、折叠导航及双 view，独立正文 / 可见行 / 完整 index oracle；只记 headless 基线，不测桌面、原生 idle 或预算通过 |
| `python3 -B scripts/perf_diff.py .pi/tmp/NEW-diff-perf` | 构建 profiler release `perf_diff`，真实 Git patch 约 256 KiB / 2 MiB，各三轮 staged / unstaged 的 headless 专项；独立正文 / 折叠投影 oracle、同场景双 view 与分段基线，不测原生 idle |
| `perf_view` / `perf_m1` | 基础布局 / 绘制与命令输入；M1 未等待输入段的 ready 只是末键恢复，不是逐键 ready 或原生持续输入 |
| `RUNS=3 target/release/examples/perf_split .pi/tmp/NEW-sample.txt` | 共享 buffer 双 view；聚合 ready 与选中 view 分开记录，不给聚合擅加旧单 view 门限 |
| `target/release/examples/perf_candidates .pi/tmp/NEW-candidate-fixture` | 候选过滤 / 导航基线，无新增门限，fixture 目录必须不存在 |
| `RUNS=3 target/release/examples/perf_syntax` | Rust / 长 raw string（8 MiB）及 TSX / HTML / Markdown（256 KiB）独立语法基线，可用 `SHAPES=rust,long-string` 等筛选；`syntax_and_layout_ready_ms` 包含解析 / 查询 / 子语言，不能套用旧纯布局 ready 门限 |
| [perf_load](../examples/perf_load.rs) / [perf_baseline.sh](../scripts/perf_baseline.sh) | 文件层基线；参数、样本和断言以工具为准 |

headless 仅作 CPU / 功能回归，不代表原生字体、平台 IME、GPU 或实际呈现；软件诊断与宿主硬件验收分开。

## 宿主 fish 入口

仅在对应批次获准后，将模式和新目录代入：

```fish
fish scripts/acceptance.fish MODE NEW_OUTPUT_DIRECTORY --consent
```

下表是可选入口索引，不是当前阶段待办。模式及参数以 [acceptance.fish](../scripts/acceptance.fish) 和对应驱动为准：

| 模式 | 用途 / 操作来源 |
|---|---|
| `perf` | M2 文件 / buffer、普通原生及 M1 持续输入矩阵；开销大，不默认全跑 |
| `display-perf` | mixed / single-ascii 原生显示、字体与输入代表性复测，不覆盖其余样本或持续输入 |
| `split-perf` | 两种分屏方向的共享 buffer 双 view 性能；不重复人工功能清单 |
| `open-diagnostic` | [单次打开诊断](#单次打开诊断)，不是完整验收 |
| `ime` | wtype / fcitx5 真实正文 IME 测量；需 Hyprland，不替代完整 minibuffer IME 观察 |
| `split-ime` | 用户按窗口提示完成正文 / minibuffer IME；需 Hyprland、grim，见下节隐私说明 |
| `manual` | 两窗口文件 / IME / OS 关闭；完整步骤见[人工文件与关闭流程](acceptance/manual.md#manual人工文件与关闭流程) |
| `candidates` | 候选、路径、buffer、字体及正文 / minibuffer IME；完整步骤见[定向人工验证](acceptance/manual.md#候选式-minibuffer定向人工验证) |
| `scripting` | 启动、脚本命令、成功 / 失败重载和默认回退；[驱动](../scripts/scripting_manual.py)分阶段打印步骤 |
| `messages` | 长 / 多行消息、读入注记、历史与编辑恢复；[fixture 工具](../scripts/m2_manual.py)打印五组步骤 |
| `themes` | 配色 / 用户覆盖、分屏、取消、读入与编辑恢复；fixture 工具打印五组步骤 |
| `syntax` | Rust 高亮、分屏、主题、历史、普通文本 / 另存与布局 / 保存；fixture 工具打印五组步骤 |
| `languages` | 一个窗口，Markdown / HTML 嵌入高亮、分屏 / 主题、两处正文真实 IME 取消 / 提交、历史及保存；fixture 工具打印五组步骤，独立核对两个“你”和配置字节 |
| `flicker` | 一个窗口，Markdown / HTML 各约 256 KiB，ASCII 连续输入 / 删除及共享显示 / 历史的配色稳定性；三组目视步骤，独立核对正文恢复及空配置，不复测 IME |
| `syntax-perf` | Markdown（含 HTML→JS/CSS）/ HTML 各约 256 KiB、各三轮原生 CPU 输入 / 绘制；布局与语法 ready 分开记录，不外推大文件全矩阵 |
| `diff-perf` / `diff-diagnostic` | F1 diff 原生 CPU / 提交专项，约 256 KiB patch 三轮，每轮一个窗口及内部左右分屏；前者超限即停，后者显式完整诊断、记录性能超限后继续，见 [专项说明](acceptance/profiles.md#f1-diff-专项性能) |
| `magit` | 一个窗口，隔离配置与临时 Git 仓库；四组 diff 显示 / 折叠导航、分屏刷新、长行水平查看、旧侧 / index / worktree 访问，见 [fixture 说明](../scripts/magit_fixture.py) |
| `magit-commit` | 一个窗口及内部分屏，独立空配置与临时仓库；临时显示 / `q`、消息与 companion / 进程焦点、取消、活动提交的 OS 关闭拒绝、保存成功 / 刷新及独立 Git / 消息检查，见 [F3 定向流程](acceptance/profiles.md#f3-最小定向验证) |
| `magit-status` | 一个窗口及内部分屏，独立空配置和临时仓库；status 可读性 / 焦点、默认 `s` / `u` 与 diff `[stale]`，每组后独立核对完整 index，见 [F2 定向流程](acceptance/profiles.md#f2-最小定向验证) |
| `styles` | 一个窗口，隔离 rich / plain 主题下的 Rust / Markdown 字体、装饰、背景与换行，用户手动做两处 IME 取消 / 提交；四组观察及独立字节核对 |
| `styles-perf` | mixed 约 8 MiB 三轮，再 Markdown 约 256 KiB 三轮；至多六个顺序窗口，复用原生 CPU 驱动，不展开全矩阵 |

人工模式先读终端提示；`scripting` / `messages` / `themes` / `syntax` / `flicker` / `magit` / `magit-status` / `magit-commit` 使用英文输入，不自动注键、不改变 IME、不截图，不测原生性能。`languages` / `styles` 由用户在指定两处手动使用日常中文输入法；脚本不注键、不切换输入法、不截图，其 IME 结论来自用户观察及保存字节，不是自动状态断言或时延测量。详细键序列只在驱动中维护，不在本文再复制。不要直接照抄历史批次目录名。

### F3 最小定向验证

`magit-commit` 的单窗口范围、分组 Git / 消息检查与停止规则见 [F3 定向流程](acceptance/profiles.md#f3-最小定向验证)。

### F2 最小定向验证

`magit-status` 的授权范围、阶段性 index 检查与停止规则见 [F2 定向流程](acceptance/profiles.md#f2-最小定向验证)。

### F1 最小定向验证

`magit` 的隔离范围、操作与独立字节核对见 [F1 定向流程](acceptance/profiles.md#f1-最小定向验证)。

### F1 diff 专项性能

`diff-perf` / `diff-diagnostic` 的动作、逐帧证据、停止策略与非验收边界见 [diff 专项流程](acceptance/profiles.md#f1-diff-专项性能)。

### E3 最小定向验证

`styles` / `styles-perf` 的先后顺序、字体 / IME 观察与代表性性能范围见 [E3 定向流程](acceptance/profiles.md#e3-最小定向验证)。

### E2 定向原生 CPU 批次

`syntax-perf` 的样本冻结、pending 输入覆盖和逐阶段校验见 [E2 定向流程](acceptance/profiles.md#e2-定向原生-cpu-批次)。

## 原生性能与诊断

自动性能模式使用合成 GPUI 输入；运行期间不要输入、移动 / 调整或关闭窗口，不冒充 OS 键盘覆盖。预算与指标只在 [performance.md](performance.md) 定义，CPU 提交不等于 GPU 执行 / 屏幕呈现；真实 GPU / 呈现测量须另行确认方案。

`split-perf` 分别记录选中 view 与同场景所有 view 的 ready：前者沿用对应预算，后者是没有新增门限的聚合基线，不能合成“多 view 整体验收通过”。改宽 / 字体走 view 层入口，不冒充正式配置命令端到端时延。字段、动作矩阵及校验以 [perf_native.py](../scripts/perf_native.py) 为准，不为整理文档放宽断言。

fish 不提供软件降级；底层 `--software` 只供另行授权的诊断。普通原生 / IME、M1 / M2 / split-perf / syntax-perf 在运行中检查各自适用的既有预算，失败停批。下述显式完整显示诊断只改变采集停止策略，不改变预算或验收结论。

### 单窗口完整显示诊断

用户另行批准后，`perf_native.py --display-diagnostic --sample mixed --runs 1 --consent` 仅运行一个 mixed 窗口的完整显示动作：打开、滚动、改宽、字体、稳定绘制、idle、点击、插入 / 删除 / 换行 / 合行及同回调 32 键。沿用普通 probe 的实际动作，不测真实 OS 输入、IME 时延或 GPU。可按以下入口执行，文件及输出目录必须是新的：

```sh
cargo build --release --features native-profiler --example perf_native
python3 -B scripts/perf_sample.py .pi/tmp/NEW-mixed.txt
python3 -B scripts/perf_native.py .pi/tmp/NEW-mixed.txt .pi/tmp/NEW-display-diagnostic --consent --display-diagnostic --sample mixed --runs 1
```

驱动自动建立并核对隔离空配置。每个完整阶段仍按原门限判定，记录全部超限及 `over-budget` 状态后继续下一动作；报告始终为 `acceptance=false`，普通验收读取入口拒绝此报告，即使所有数字合格也不自动算验收。流程完整采集的 `completed=true` 与 `budget_passed` 分开，不能将成功退出解释为预算通过。

正文 / marked 状态、可见性、完整索引、实际滚动和字体等正确性检查仍为硬停止。原生阶段观察上限为 2 秒；父驱动另有阶段 watchdog，以便 UI 阻塞时也能终止，开始收到阶段进度前最多等 5 秒。这些是诊断安全上限，不是性能预算。阶段结果即时刷盘；若父驱动终止阻塞进程，保留已有 CSV / stdout / stderr，当前阶段可能没有完成行或 footer，未观测边界不补 0。超时或正确性失败不得自动重跑；先交回新目录分析。

<a id="首轮超限后的单次诊断"></a>

### 单次打开诊断

`open-diagnostic` 只运行 scratch 准备和一次 mixed 文件打开，`acceptance=false`；成功不自动继续矩阵。保留主 CSV、frames / views / preparation 侧报告与 diagnostic.json，失败也交回分析。字段见 [诊断实现](../examples/support/native_diagnostic.rs) 和驱动，不重述历史失败过程。

## 真实 IME 与截图隐私

只有 `ime` 自动注入真实 OS 按键：需 wtype、fcitx5-remote，grim 可选；期间不要操作键鼠，驱动检查目标焦点并恢复原焦点 / 输入法。注入流程与人工真实 IME 观察不能互相冒充。

`split-ime` 不注键、不代改输入法，用户在右正文、左正文、minibuffer 依次按提示完成“ni → Escape，再 ni → 提交你”，等待下一步，不做额外编辑 / 缩放 / 关闭。编辑器准备动作使用正式路由的合成 GPUI 键，不是 OS 键盘覆盖。候选不是“你”时明确选“你”，不改 fixture 预期。结束 / 失败时尝试恢复原焦点，原窗口已关闭则如实记录，不转而修改其他窗口输入法。

该模式的六张候选截图覆盖测试所在的**整块显示器**，可能包含其他窗口；征求同意前必须说明，运行前隐藏敏感内容。`result.json` 只确认自动集成与字节断言，候选窗视觉位置仍须离线审图；不扩展为原生时延、GPU 或完整编辑矩阵通过。

## manual：人工文件与关闭流程

两个隔离窗口的文件闭环、正文 / minibuffer IME 与 OS 关闭步骤见 [人工文件与关闭流程](acceptance/manual.md#manual人工文件与关闭流程)。

## 候选式 minibuffer：定向人工验证

`candidates` 的候选 / 路径 / buffer、字体与窄窗口、正文 / minibuffer IME 六组操作见 [候选式读入流程](acceptance/manual.md#候选式-minibuffer定向人工验证)。

## 结论与证据边界

- 只有获批、实际覆盖的范围才可写为通过；具体窗口尺寸、字体或观察未提供时如实保留，不用推测补齐。
- 自动成功不证明全部视觉 / 平台行为正确；功能、原生 CPU 时延、真实 IME 和 GPU / 呈现分别解释，不能互相替代。
- 本文不记某批次已通过，也不产生新的测试授权；冻结记录不随工具或流程整理改写，必要时只修复引用链接。
