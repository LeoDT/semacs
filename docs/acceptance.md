# 验收流程

本文维护验证范围的选择、本地 / 宿主分工、可执行入口与安全规则。行为见 [保真度契约](emacs-fidelity.md)，预算与指标见 [性能预算](performance.md)，阶段进度及历史证据从 [计划](plan.md) 和 [文档导航](README.md#按需参考与冻结记录) 查阅，不在本文逐阶段累积覆盖清单或批次结论。

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
| 人工 fixture / 字节核对工具 | `python3 -B -m unittest discover -s scripts -p 'test_m2_manual.py' -v`；Magit profile 另跑 `python3 -B -m unittest discover -s scripts -p 'test_magit_fixture.py' -v` |
| 脚本配置冒烟驱动 | `python3 -B -m unittest discover -s scripts -p 'test_scripting_manual.py' -v` |
| split IME 驱动 | `python3 -B -m unittest discover -s scripts -p 'test_split_ime.py' -v`，不访问桌面 |
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
| `manual` | 两窗口文件 / IME / OS 关闭；完整步骤见[人工文件与关闭流程](#manual人工文件与关闭流程) |
| `candidates` | 候选、路径、buffer、字体及正文 / minibuffer IME；完整步骤见[定向人工验证](#候选式-minibuffer定向人工验证) |
| `scripting` | 启动、脚本命令、成功 / 失败重载和默认回退；[驱动](../scripts/scripting_manual.py)分阶段打印步骤 |
| `messages` | 长 / 多行消息、读入注记、历史与编辑恢复；[fixture 工具](../scripts/m2_manual.py)打印五组步骤 |
| `themes` | 配色 / 用户覆盖、分屏、取消、读入与编辑恢复；fixture 工具打印五组步骤 |
| `syntax` | Rust 高亮、分屏、主题、历史、普通文本 / 另存与布局 / 保存；fixture 工具打印五组步骤 |
| `languages` | 一个窗口，Markdown / HTML 嵌入高亮、分屏 / 主题、两处正文真实 IME 取消 / 提交、历史及保存；fixture 工具打印五组步骤，独立核对两个“你”和配置字节 |
| `flicker` | 一个窗口，Markdown / HTML 各约 256 KiB，ASCII 连续输入 / 删除及共享显示 / 历史的配色稳定性；三组目视步骤，独立核对正文恢复及空配置，不复测 IME |
| `syntax-perf` | Markdown（含 HTML→JS/CSS）/ HTML 各约 256 KiB、各三轮原生 CPU 输入 / 绘制；布局与语法 ready 分开记录，不外推大文件全矩阵 |
| `diff-perf` / `diff-diagnostic` | F1 diff 原生 CPU / 提交专项，约 256 KiB patch 三轮，每轮一个窗口及内部左右分屏；前者超限即停，后者显式完整诊断、记录性能超限后继续，见 [专项说明](#f1-diff-专项性能) |
| `magit` | 一个窗口，隔离配置与临时 Git 仓库；四组 diff 显示 / 折叠导航、分屏刷新、长行水平查看、旧侧 / index / worktree 访问，见 [fixture 说明](../scripts/magit_fixture.py) |
| `styles` | 一个窗口，隔离 rich / plain 主题下的 Rust / Markdown 字体、装饰、背景与换行，用户手动做两处 IME 取消 / 提交；四组观察及独立字节核对 |
| `styles-perf` | mixed 约 8 MiB 三轮，再 Markdown 约 256 KiB 三轮；至多六个顺序窗口，复用原生 CPU 驱动，不展开全矩阵 |

人工模式先读终端提示；`scripting` / `messages` / `themes` / `syntax` / `flicker` / `magit` 使用英文输入，不自动注键、不改变 IME、不截图，不测原生性能。`languages` / `styles` 由用户在指定两处手动使用日常中文输入法；脚本不注键、不切换输入法、不截图，其 IME 结论来自用户观察及保存字节，不是自动状态断言或时延测量。详细键序列只在驱动中维护，不在本文再复制。不要直接照抄历史批次目录名。

### F1 最小定向验证

`magit` 使用一个窗口及其内部分屏，检查样式 / 折叠指示、导航、共享刷新、截断长行与 RET 的适当版本访问；详细键序列只在 `scripts/magit_fixture.py` 维护。驱动为测试子进程设置独立 `XDG_CONFIG_HOME`，清除继承的 `GIT_*` 覆盖并禁用全局 / 系统 Git 配置；不修改父终端环境、日常配置或仓库。

fixture 准备只在新输出目录建立两份 tracked 文件及确定的 HEAD / index / worktree 差异。结束后独立核对正文 / init.js 字节、HEAD 身份、index / Git 配置字节及两侧 blob；不把人工回答 yes 代替字节检查。任一失败停止，不修复 fixture 或自动重试。没有截图、自动注键、输入法变动或原生性能测量；原生颜色 / 字体 / 几何来自人工观察，不记作 GPU / 呈现时延或完整性能矩阵通过。

### F1 diff 专项性能

先完成本地专项工具自检，再对具体原生批次征求同意。`diff-perf` 顺序打开三个窗口；每轮复用同一临时仓库，按 staged / unstaged 覆盖打开、首 hunk 定位与折叠 / 展开、长改动行导航、垂直滚动、左右分屏、共享折叠 / 刷新 / 展开，以及两段各 30 个无主动 invalidation tick。刷新重读不变的磁盘 patch；不测改宽、字体、输入、真实 IME 或 OS 键盘。运行期间不要键入、改宽或关闭 probe。

仅使用合成 GPUI 动作，不自动注入 OS 按键、不修改输入法、不截图。驱动给子进程设置空的独立 `XDG_CONFIG_HOME`，隔离 Git 全局 / 系统配置与继承的 `GIT_*`，不操作日常仓库。全部 fixture / HEAD / index / Git 配置、独立正文及投影 oracle 和 manifest 在测试前冻结；每轮前后检查源码 / 二进制 / fixture / 配置。构建强制使用仓库 target 目录，摘要实际受测 example；输出保存 HEAD 相对的全部 tracked 差异及 untracked 源码。

逐帧核对真实目标 buffer / 正文版本、当前折叠投影、兼容布局及同场景双 view 就绪，不能用旧正文提交代替刷新完成；计时后另核对完整正文及独立投影 oracle。原生保留实际适用的 draw / 垂直滚动 / idle 门限，其余 diff 时延无新增门限，只作基线。30 秒阶段观察上限及父进程 watchdog 是资源保护，不是放宽后的性能预算。默认 `diff-perf` 任何超限、正文 / 投影 / 身份错误或异常立即停批，不重试或修复 fixture。

用户另行批准的 `diff-diagnostic` 沿用相同 small 三轮和动作，不在 setup 超限时提前结束：窗口启动、命令准备及动作的实际帧全部写入 `*.frames.csv`，记录 draw、request-layout / prepaint / paint / other、CPU submit、同帧正文几何与 display 身份；setup 不冒充动作时延。性能超限逐帧 / 逐动作标记并继续本批剩余动作和窗口，idle 产生 draw 也保留原预算失败，不填造 idle 时延。报告 / 环境明确 `diagnostic=true`、`acceptance=false`；`completed=true` 只表示采集完成，`budget_passed` 按全部适用证据独立判定，普通读取入口拒绝诊断报告。驱动交叉核对逐帧数据、动作统计和提交边界，setup 超限也不能漏记到总体判定外。

两种模式的正文 / 投影 / 几何 / 身份错误、异常、观测丢失与安全超时仍停止后续动作 / 窗口，保留部分 CSV / frames / spans、stdout / stderr 及停止原因，不重试或修复 fixture。父进程按窗口启动、阶段准备与实际动作的进度刷新 watchdog，不因 UI 卡住而无限等待，也不把安全超时当成放宽后的预算。CPU 提交仍不证明 GPU / 屏幕呈现。

### E3 最小定向验证

先运行 `styles`，全部人工观察与磁盘字节检查通过后，才运行独立新目录的 `styles-perf`。详细操作仅由 fixture 工具打印，不在这里重复维护。

`styles` 只打开一个窗口，用可丢弃 Rust / Markdown 和隔离配置检查随包默认 Lilex 的粗斜体 / 装饰、范围背景、选区 / 光标、换行与候选坐标。用户手动切换输入法并在两份正文首行各提交一个“你”，Rust 还执行 undo / redo；工具核对最终字节与配置不变。记录窗口尺寸、字体效果和输入法名称；看不出变体时先记录并停止，不自行改字体重试。没有截图、OS 注键或自动输入法切换，不测 IME 时延。整行背景原语仍以本地几何回归为证据，不把文字范围背景的目视检查说成整行背景宿主验收。

`styles-perf` 先用约 8 MiB mixed 覆盖普通文本的完整字体测量 / 换行，再用带真实 Markdown 语言身份的约 256 KiB 样本覆盖强调字体与嵌入高亮，各三轮；并未测试 HTML 窗口、全部 8 MiB 形状或持续输入。最多六个窗口顺序运行，期间不要输入、改宽或关闭 probe。两段均使用隔离空配置；普通 mixed 在当前阶段检查既有首场景 / 当前正文 / 完整 ready 与 draw 预算；超限即保存含失败阶段的部分报告并非零退出，不推进下一动作或后续窗口。未观察到的提交边界留空，已提交的时刻不因回调迟到而改成观察时间；Markdown 段沿用下节逐阶段检查、冻结 fixture 和 pending 输入覆盖校验。布局与语法整体就绪继续分开，不新增或放宽预算，不冒充 GPU / 实际呈现测量。

### E2 定向原生 CPU 批次

`syntax-perf` 使用隔离空配置和带 `.md` / `.html` 路径的 buffer，六个窗口依次运行；不接受无语法路径的普通文本作为 E2 证据。两份样本生成时写入统一 SHA-256 manifest，fish 将同一 manifest 摘要交给两种样本的驱动；每轮启动前 / 完成后核对整批两份文件及 manifest，不能在后一个样本启动时重新建立变更后的基线。输入位置由完整 script marker（Markdown 还包含围栏上下文）定位到嵌入 JavaScript 的 `call(42)` 前，测插入 / 删除、换行 / 合行、同回调 32 键插入 / 删除，随后 30 次稳定强制重绘及 30 个无主动失效 tick。只使用合成 GPUI 输入；运行期间不要键入、改变窗口或关闭 probe。

逐帧核对精确 display 目标、当前 committed 语法快照、正文可见和索引覆盖；报告单列首次提交、布局 ready、观察到的语法 + 布局 ready。中间版本可在布局就绪后被下一次编辑取代，未观察到的语法 ready 留空，不填 0；最后一次编辑须等待当前快照语法完成后才进入稳定绘制 / idle。报告记录动作前语法 pending 状态；若整轮没有在前一版本语法仍 pending 时输入，按覆盖缺失停止，不冒充测到实际 worker 线程并行或自动调整样本重测。

已适用的输入 / CPU 提交 / draw / idle 门限不变，语法整体就绪没有新增数值门限，也不套旧纯布局门限。超限、帧 / 身份缺失、字节错误、未取得 pending 输入覆盖或配置 / fixture 变化均停止后续窗口并保留失败报告。此代表性批次不替代 8 MiB 六类矩阵、持续输入、真实 IME 时延或 GPU / 实际呈现证明。

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

两个窗口均使用本批独立空配置和默认键位，不读取日常 `init.js`。

### 第一个窗口

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

### 第二个窗口

1. 初始打开 close.txt，在开头输入大写 `Z`；`M-x` 输入未完成的命令名 `for`。
2. 使用平时合成器的 **关闭窗口快捷键 / 标题栏关闭按钮**，不要用 `C-x C-c` 代替 OS 关闭。当前实现应取消读命令会话并出现保存询问，而不是立即丢数据。
3. 按 RET，不能默认同意或关闭；再 `C-g` 取消。窗口必须保留，Zclose 仍在，焦点恢复正文。用 C-f / C-b 检查即可。
4. 再触发 OS 关闭，在保存询问按单键 `n`。应继续询问是否放弃仍修改的 buffer；输入完整 `no RET`，取消退出，文字仍保留。
5. `C-x C-s`，等待 clean，然后再触发 OS 关闭，应直接退出。脚本独立检查 close.txt 为 `Zclose\n`。

若平台只能走已批准的窗口级确认例外，记录实际流程、安全默认取消与焦点情况，先回报，不直接把不符上述预期的步骤勾成通过。

## 候选式 minibuffer：定向人工验证

本项使用 `candidates` 模式及本批独立空配置，初始默认键位、字号 16px；不自动注键，不测性能。不将既有 `manual` 模式的结果当作本项通过。

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

- M-x `set-font-family` RET，输入本机已安装的一个具体字体族名后 RET（不使用 `monospace` / `sans-serif` 通用别名）。按实际显示观察字体变化；系统字体回退不算字体实际命中的证明。脚本另保存 fc-match 环境信息，但它不代表随包默认字体或 GPUI 的实际命中。
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
- 可用 M-x `reset-font-size` 恢复 16px，再 `set-font-family` 恢复 `Lilex`；这些设置仅本次运行有效。
- C-x C-c 退出。全部测试 buffer 应 clean；不要额外改字。脚本询问上面六组观察以及窗口尺寸 / 字体说明，并独立核对新文件、未覆盖的原文件和所有子目录 fixture 的字节。任何未观察项目均答 no。

## 结论与证据边界

- 只有获批、实际覆盖的范围才可写为通过；具体窗口尺寸、字体或观察未提供时如实保留，不用推测补齐。
- 自动成功不证明全部视觉 / 平台行为正确；功能、原生 CPU 时延、真实 IME 和 GPU / 呈现分别解释，不能互相替代。
- 本文不记某批次已通过，也不产生新的测试授权；冻结记录不随工具或流程整理改写，必要时只修复引用链接。
