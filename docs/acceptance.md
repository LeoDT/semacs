# M2 / M3 宿主验收

预算与证据边界见 [性能预算](performance.md)，产品行为以 [Emacs 保真度](emacs-fidelity.md) 为准。本页是执行说明，不代表通过记录。

## 分工与执行顺序

隔离环境负责 Rust / Python / Emacs 参照、headless 回归及工具自检。原生窗口、硬件字体 / CPU 提交、真实 Wayland / IME 和 OS 关闭须在用户宿主执行；没有 `/dev/dri` 不回退到软件渲染冒充通过。

首屏修复后，正式 `perf` 矩阵已由用户执行并完成逐项核对，产物 `.pi/tmp/m2-host-perf-after-layout/`，结果与尺寸可比性见 [收尾记录](records/m2-m3-local.md#修复后的正式宿主性能矩阵)。不要覆盖该目录或原样重跑。

另行授权的真实 IME 代表性复测也已执行并核对，产物 `.pi/tmp/m2-host-ime/`，详见 [IME核对记录](records/m2-m3-local.md#正式宿主-ime-代表性复测)。不要覆盖或原样重跑；自动结果和截图抽查不代替完整人工观察。

另行授权的人工验收也已由用户完成并核对，产物 `.pi/tmp/m2-host-manual/`；五组观察与四个文件精确字节通过，最终结论见 [验收记录](records/m2-m3-local.md#阶段最终结论)。本轮三类入口均已完成，不需要原样重跑。

以下仅为**已执行命令的记录**：

```fish
fish scripts/acceptance.fish manual .pi/tmp/m2-host-manual --consent
```

以后如需复测，须另行取得用户同意，在仓库根目录的正常 **fish / Wayland 宿主终端**执行，并使用尚不存在的新输出目录。`manual` 会自动生成 `OUTPUT/files/` 下的 alpha / beta / overwrite / close 文件并打开窗口，不需要自行准备fixture；不能覆盖旧报告。

按顺序执行，任一步失败就停止，回传输出目录，不自动重测。脚本保留构建日志、版本 / 差异、环境、逐项 CSV、摘要和失败原因，不删除产物。原生驱动还记录二进制 / 驱动 SHA-256、字体和 fixture 摘要。结束后将目录路径交回核对，不只报告退出码。

- `perf` 不注入 OS 按键，但会打开测试窗口。不要输入、关闭或调整它们；先测 mixed 的 M2 三轮，正常才继续其余矩阵。整批约数分钟，其中 M1 持续输入固定六分钟；M2 fixture 产物约 600 MiB。
- `ime` 会用 wtype / fcitx5 Pinyin 注入真实输入，需 Hyprland、wtype、fcitx5-remote；grim 可选，用于候选截图。测试期间不要操作键鼠。驱动在注键前检查焦点，结束时恢复原焦点和输入法状态。
- `manual` 不自动注键，打开两个正式 semacs 窗口，按下节操作。每组需在终端明确输入 `yes`，最终还独立核对磁盘字节；未观察到的项目不可答 yes。

硬件门限适用前仍须核对与 R1 / M1 的硬件、字体、尺寸、构建可比性。环境不同先报告，不擅自改变阈值。

## 首轮超限后的单次诊断

完整验收在首个 mixed 文件打开时超预算停止；子调用诊断将主要停顿定位到同步换行预览的一次未缓存文本测量，早于 wrap worker 启动。实现已将新文件的实际首屏索引 / shaping 准备移至后台，就绪前保留原正文；没有改预算，也未判定 GPUI 内部存在缺陷。

修复后的单次 mixed 宿主核对已完成，draw最大3.142005 ms、目标首帧prepaint0.117187 ms、无空白，首屏提交70.676942 ms、完整就绪提交234.461781 ms。仅支持该样本的draw修复有效，不证明整体打开变快或完整验收通过。详见 [本地收尾记录](records/m2-m3-local.md#修复后的单次宿主核对)。

保留 `.pi/tmp/m2-host-perf/`、`.pi/tmp/m2-open-diagnostic/`、`.pi/tmp/m2-prepaint-diagnostic/` 与 `.pi/tmp/m2-layout-fix-diagnostic/`。以下为**已执行命令的记录，不要原样重跑**：

```fish
fish scripts/acceptance.fish open-diagnostic .pi/tmp/m2-layout-fix-diagnostic --consent
```

此入口只执行 scratch 准备与一次正式文件打开，不继续其他正文、编辑 / 保存或完整矩阵，不自动重试。未来如需再诊断，须另行征得用户同意并使用尚不存在的新目录。仍保留原 fixture 配方和准备方式，不预热目标正文；既有预算不变，超限会非零退出，这也是有效的诊断产物。

`open-diagnostic/mixed-1.*` 包含：

- `csv`：两步聚合报告，ready 未观察到时保留空值。
- `frames.csv`：包括 setup 的逐帧 request-layout / prepaint / paint、draw 剩余成本、平台提交及提交前间隔；携带目标身份匹配、正文尺寸、扫描范围、布局就绪等状态。
- `views.csv`：最慢帧所属 view 的阶段与 self / total 时间。profiler 只保留 self ≥0.5 ms 的条目；嵌套 total 会重叠，不能相加当作整帧成本。
- `preparation.csv`：首屏嵌套调用（prepare / ensure-index、字体初始化、wrap-preview、可见行 / 单行 shaping、未缓存 grapheme 测量）及 initial / wrap / shape worker 区间。`parent` 关联调用树，只有前台区间关联 `frame`；worker 以同一时间轴观察重叠，不强塞入某个帧。`bytes` 是输入字节数（wrap-preview 为扫描上限，其余无对应输入时为0），不是新增扫描量。无正文内容落盘。
- `diagnostic.json`：核对逐帧与聚合值后提取最慢打开帧、慢 view、首屏子调用分组及与该帧重叠的 worker；另外列出所有 initial-worker 区间，包含目标首帧之前的准备耗时；预算失败时同样生成。旧版没有子调用数据时 `preparation=null`，不补造证据。
- stdout / stderr 与 environment：含版本、二进制 / 驱动摘要、恢复状态及停止原因。

数据先缓存在内存，结束后写盘，避免在被测 draw 内输出文件。子调用记录仅在 `native-profiler` 构建且单次诊断显式启用时采集；默认构建不包含计时钩子。最多保留16384个区间，溢出作无效报告；后台只记录粗粒度区间，不逐 grapheme 记录。停止时仍运行的 worker 标为 `complete=false`，end 是观察截止而非完成时刻。

嵌套子调用耗时不能相加作为整帧成本；worker 重叠只说明并行发生，不证明锁等待。计时、内存分配与短临界区也有诊断开销，不从一次诊断宣告性能通过。CPU draw 剩余成本不能直接归因于 GPU；分阶段时间包含各阶段内可能的等待，不是 CPU 周期。没有 GPU timestamp 或屏幕呈现反馈。

诊断模式明确 `acceptance=false`。即使这次未复现超限，也不能据此撤销原失败、宣告 M2 通过或继续后续矩阵；将输出目录交回分析。

## 自动矩阵与解释

### M2：六类正文，各三轮

复用 `perf_native --m2-directory NEW_DIRECTORY`，由 `scripts/perf_native.py --m2` 管理，每轮 31 步：

- 从正式 `C-x C-f` 确认路径，依次读入三个约 8 MiB 文件并完成布局。
- buffer 切换、编辑、保存后下一 frame-demand tick 继续输入，再保存最终版本；独立核对请求快照、dirty 与磁盘。
- 七次已打开 buffer 的切换，记录提交与完整就绪。
- 2000 项目录补全，以及 TAB 后继续输入、取消并重开读入会话。
- 关闭三个文件，再做三轮打开 / 编辑 / 保存 / 关闭；记录 RSS、buffer 数与已关闭 view 的弱引用存活情况。

测量起点为最终 RET / TAB，而非完整路径逐字输入。准备 prompt、预生成 fixture、全文断言和磁盘断言不计入动作区间。文件缓存不受控，不宣称冷磁盘读取。

- 打开 / 切换的 first / ready 要求目标显示身份、兼容布局及完整索引，不能由旧 buffer 的保留正文冒充。
- `save-edit` 的 first / ready 从下一 tick 的 y 输入开始；`save_observed_ms` 从保存命令开始到 UI 可观察完成。记录 `save_ack_before_edit`：如果为 true，保存已在该键之前确认；false 也只说明尚未观察到确认，不证明 worker 正在执行磁盘 I/O。这不是长时间写盘并发输入压测。
- 普通保存只报告 UI 可观察完成；补全 / 取消只报告 UI 结果观察，不填目标提交时延。
- 只对已有输入 2 / 16 / 300 ms 与 draw 16 ms 预算判定；**不为 M2 打开、保存、切换或补全凭空设门限，也不套用旧的预载正文冷窗口门限**。
- RSS 为短周期趋势，不是内存泄漏判定；弱引用消失只证明对应 view 不再存活，不证明 GPU / allocator 缓存全部回收。

### 既有性能回归

普通原生预算：六类各三轮，包含打开预载正文窗口、滚动、改宽、字体、强制重绘、空闲、四类输入及 32 键批量。驱动逐行复核既定预算，任一运行失败停止后续矩阵。普通 / IME 模式在每份窗口报告完成后判定；M1 / M2 还会在测量过程中检查既有预算。

M1：mixed 与 single-ascii 各三轮、60 秒 / 50 ms。它是当前版本的代表性回归，**不替代历史六类完整 M1 矩阵，也不代表其他四类完成了本轮持续输入复测**。过期目标 ready 继续按批准口径保留未测。

IME：mixed 与 single-ascii 各三轮，覆盖正文预编辑、取消、提交、滚动 / 改宽 / 字体 / 点击后的组合。它不包含完整 minibuffer IME，后者由人工补齐。

所有时间仍是 CPU / 平台提交或 UI 观察，不是 GPU 执行或实际屏幕呈现。

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

## 验收仍须人工核对的边界

- 自动脚本成功不证明候选窗口肉眼位置正确，也不证明每个 F1 / F2 / F3 项均已核对。
- 真实用户按键延迟、GPU timestamp、实际屏幕呈现、长时间内存趋势与未覆盖平台 / 键盘布局不由这些入口推断。
- 通过、失败与未测项须结合本地回归、宿主环境、逐项报告和用户观察汇总，不仅凭脚本退出码判断。本轮结论见 [验收记录](records/m2-m3-local.md#阶段最终结论)。
