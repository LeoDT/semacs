# M2 / M3 验收记录

**M2 / M3 按既定 v1 范围验收通过**，最终证据汇总与保留限制见「阶段最终结论」。下文按顺序记录 2026-11-01 的初轮本地验证及后续宿主诊断、首屏布局修复、正式性能矩阵、真实 IME 与人工验收；较早段落的未完成 / 未授权表述指各自当时状态，不是当前结论。执行步骤见 [宿主验收](../acceptance.md)，预算见 [性能预算](../performance.md)。

## 本地结论

- 现有 M2 headless 功能回归足以覆盖文件字节 / 失败安全、保存排队与并发编辑、跨 buffer、读入取消 / 迟到结果及键盘闭环；没有另建重复的功能测试矩阵。
- 补充的是测试工具本身：原生 M2 31 步驱动的正式 Root / shell / paint 探针 headless 自检、目标身份与测量边界、报告预算复核、失败停止、人工 fixture 与独立磁盘字节校验。
- 原生普通 / IME 报告此前只检查覆盖与完成，本轮补齐逐行既有预算核对，不能只由 completed=true 推断性能通过。
- 最初收尾验证未修改产品 `src/` 行为或预算阈值；后续产品修改见下文首屏布局修复。原生探针改为观察 shell，以在切换 buffer 后继续采集当前正文，而非只订阅初始 editor。
- 首轮工具交付时默认 Rust **312 passed**；native-profiler **329 passed**；Python **28 passed**。两组 clippy `-D warnings`、fmt、fish 语法与 diff 检查通过。
- GNU Emacs 31.1 的 M1 / M2 ERT **37/37** 通过；另运行六个 M2 M-x pty 参照用例。Emacs 结果只是外部行为参照，不冒充产品 GUI 通过。

## 当前版本 headless 性能

release、默认 feature、TestPlatform / TestTextSystem；六类约 8 MiB 正文各三轮，软换行开启，沿用原尺寸 / 字体。fixture SHA-256：

`1bbb39412c50be06cda0898f541587c1bda65d801c223d6f4d1a9269a6e5c235`

`perf_view` 18 行，`perf_m1` 540 行，完整覆盖与文本 / point / 索引断言通过。逐行判定，没有用平均值掩盖运行超限；forced / empty 仍按每轮原有均值指标比较。单位 ms：

| 指标 | 最大值 |
|---|---:|
| 冷首帧 | 0.424 |
| 初始 / 改宽 / 字体完整就绪 | 162.457 / 163.885 / 162.043 |
| 滚动更新 | 0.193 |
| forced frame / empty draw（每轮均值） | 0.059 / 0.058 |
| 四类输入 front / ready | 0.467 / 166.920 |
| M1 同步 / front / ready | 0.609 / 0.727 / 162.237 |

全部满足已有 headless 预算。M1 有界连续段只代表逐键前台成本及末键恢复，不代表原生持续输入；`perf_view` 保留原历史窗口边界，`perf_m1` 与 M2 驱动自检使用正式 Base Root。M2 自检的合成 profiler 帧只验证工具状态机，**不产生原生时延证据**。

## 产物

- `.pi/tmp/m2-acceptance/{perf_view,perf_m1}.csv`、对应 stderr 与 `headless-check.json`。
- `.pi/tmp/m2-final-{tests,profiler-tests,clippy,profiler-clippy,fmt}.log`。
- `.pi/tmp/m2-python.log`、`m2-workload-test.log`、`m2-final-emacs.log`、`m2-final-emacs-mx.log`。
- 正式可复现入口为仓库中的 examples / scripts；临时核对脚本和日志不作为正式测试的唯一实现。

## 首轮宿主停止与诊断交付

宿主 `.pi/tmp/m2-host-perf/m2-mixed/` 的 mixed 第一轮在首个文件打开时停止，后续矩阵未运行。release / RADV / Mesa 26.2.5，fixture 摘要与原配方一致，字体 Noto Sans CJK SC；正文为 847×1306，与历史 M1 的 838×1306 不完全相同。

- 同步确认动作 0.044 ms；两次 draw 中最大 **41.566 ms >16 ms**；无空白正文帧，完整 ready 未观测。
- 原报告 first_submit_ms 为 59.220 ms。审查诊断工具时发现，异步打开尚未激活目标时可能将旧正文计入 first / ready；已修正并补回归。该时延须由诊断版重新确认；此工具问题不改变 draw 计时边界，不能撤销 41.566 ms 的预算失败。
- stderr 为空，completed=false、restored=true。保留原始产物，不调宽预算，不修改产品代码，未自动重测。
- 已交付只运行两步的单次打开诊断，记录原生 profiler 的帧阶段与慢 view，不增加目标预热、额外热重绘或产品计时钩子。诊断数据在内存缓冲，结束后输出；范围与入口见 [宿主验收](../acceptance.md#首轮超限后的单次诊断)。
- 诊断工具检查：默认 Rust **312**、profiler **331**、Python **33** 项通过，两组 clippy / fmt / fish 语法 / diff 检查与 release 构建通过。日志前缀 `.pi/tmp/m2-diagnostic-`。
- 宿主已执行单次诊断，再次因 draw 40.587240 ms 超限停止，来源 `.pi/tmp/m2-open-diagnostic/open-diagnostic/`。没有 journal 丢失，stderr 为空，restored=true。
- 目标首帧 request-layout / prepaint / paint / draw 其他成本分别为 **0.145119 / 37.749561 / 2.667160 / 0.025400 ms**；平台提交 **0.655524 ms**。主要耗时约93%落在 prepaint，慢 view 为 `semacs::view::BufferView`，其 prepaint self **37.427793 ms**。
- 此帧目标身份匹配，扫描2966字节、索引2840字节、可见56行，完整布局尚未就绪；first-submit **58.160 ms**。此前 scratch 帧 target-match=false，未被当作目标首帧。本轮正文864×1306，不与初次847×1306或历史838×1306混作完全同条件比较。
- 静态检查确认首屏字体测量 / 换行预览 / 可见行 shaping 都在该 prepaint 路径；当前证据不能精确区分这些调用与原生文本系统等待 / 后台竞争。阶段瓶颈已定位，具体根因与修复方案仍待细分，不宣称已解决。
- 未修改产品或预算，也未继续完整矩阵；诊断不算验收通过。

## prepaint 子调用诊断准备（修复前）

后续已授权继续产品定位与修复；此时尚未实施调度 / 布局优化，也没有调整预算或运行新的宿主测试。静态检查发现完整 wrap worker 在可见行 shaping 之前启动，GPUI 0.8.1 依赖的文本 shaping 路径有共享锁；这是待验证的竞争点，不是已确认根因。

`src/preparation_trace.rs` 及 `src/view.rs` / `src/display.rs` 增加 feature-gated、显式启用且有容量上限的嵌套子调用记录。单次打开工具增加 `preparation.csv`，区分字体初始化、预览、可见 shaping 与未缓存 grapheme 测量，并记录 worker 的实际开始 / 完成或观察截止。默认产品构建不包含这些钩子，profiler 普通验收模式不启用采集；没有改动正文显示、换行、缓存或任务启动顺序。

本轮默认 Rust **312**、profiler **333**、Python **36** 项通过；两组 clippy、fmt、fish 语法、diff 检查和 release 工具构建通过。日志前缀 `.pi/tmp/m2-prepaint-`。新增回归覆盖有界采集、嵌套与跨线程、未完成 worker 的截止状态、正式 shell 中子调用采集，以及新旧报告和损坏关联的校验。

宿主入口与解释边界见 [单次诊断](../acceptance.md#首轮超限后的单次诊断)，再次执行仍须用户同意且使用新目录。下一步先据细分证据选择修复，不将 headless 字体结果外推为原生根因，也不把 worker 重叠直接当作锁竞争证据。

## 子调用宿主结果与首屏布局修复

来源 `.pi/tmp/m2-prepaint-diagnostic/open-diagnostic/`，目标帧尺寸864×1306、draw **33.764130 ms**，仍超16 ms。换行预览 **20.779104 ms**，71次未缓存 grapheme 测量合计 **20.719143 ms**，其中单次3字节测量 **18.872778 ms**；可见56行 shape-line 合计 **9.559533 ms**。字段为嵌套墙钟计时，不能相加。这次主要停顿早于 wrap worker 启动，排除它导致该次停顿的猜测；后续 shaping 是否受竞争影响未知。调用进入 GPUI shape_line 不证明库缺陷，也没有证实字体 fallback 等内部原因。stderr为空、restored=true、无丢失，未继续完整矩阵。

用户据此批准修复同步首屏准备边界。`src/view/initial.rs` 在后台生成实际使用的预览索引、字体测量缓存及可见 shaped rows；超长 grapheme 导致有界预览没有完整行时，在后台等待有效索引，不激活空白目标。剩余全文索引复用原有异步完成路径。

`src/app/documents.rs` 在读入后暂存未挂载 view，由正文容器的实际 prepaint bounds 发起准备，不猜测窗口装饰或 minibuffer 高度。就绪前继续显示原正文，未将暂存 view 加入 buffer 集合；发布检查目标、origin、布局设置、颜色与窗口尺寸，过期结果丢弃 / 重算，新读入会话、切换或退出取消准备。正式激活前才安装新 view 的输入生命周期，并再次核对文件身份，避免产生重复或幽灵 buffer。此过程不是额外预热，首帧直接消费后台结果。

本次接入正式异步 visit-file；预载窗口首次创建、已有 buffer 改宽 / 换字体及普通编辑仍走原路径，不宣称所有冷字体调用均已移出 draw。尚未实现多字体或 Tree-sitter，也未修改任何预算。

本地验证：默认 Rust **320**、profiler **341**、Python **37** 通过；两组 clippy、fmt、fish语法、diff检查及 release 产品 / 原生工具构建通过。新增回归证明后台 rows 在首次 prepaint 中以同一 Arc 复用，目标 prepare 中不再发生测量 / shaping；覆盖过期文本 / 会话 / 字体 / 宽高 / 颜色、取消、空正文和超长 grapheme、改宽重算及重复文件身份。

release headless 六类各三轮的 `perf_view`18行 / `perf_m1`540行全部满足原预算，最终独立运行产物为 `.pi/tmp/m2-layout-performance-isolated/`，日志前缀 `.pi/tmp/m2-layout-`。另保留了与本地构建并行的初次 headless 结果 `.pi/tmp/m2-layout-performance/`，不用于性能改善比较。headless 只作回归，不证明原生性能；修改后的单次宿主核对如下，仍不代表 M2 / M3 整体验收通过。

## 修复后的单次宿主核对

来源 `.pi/tmp/m2-layout-fix-diagnostic/open-diagnostic/`。相同 fixture / 字体、release / RADV Mesa26.2.5、正文864×1306；completed=true、restored=true，stderr为空，journal与子调用记录无丢失。仅mixed一次，`acceptance=false`，未继续矩阵。

- 四个 measured open 帧 draw最大 **3.142005 ms**，均满足已有16 ms门限；目标首帧 prepaint **0.117187 ms**（修复前该次为30.633085 ms），paint **2.944201 ms**。目标 `BufferView.prepare` **0.004710 ms**，调用树仅有 ensure-index与缓存检查，没有前台字体测量 / wrap-preview / shape-line。
- 实际 initial-worker **37.066081 ms**，在目标首帧draw之前完成。冷计算并未消失，而是移出 draw 并被首帧复用；全文wrap worker随后运行 **174.927537 ms**，与首帧重叠，但未重现此前draw超限。
- 目标首屏仍为56行、已索引2840字节，blank_frames=0；末帧完整索引8388648字节、布局兼容且ready。旧scratch帧没有计作目标first / ready。
- first-submit **70.676942 ms**、ready-submit **234.461781 ms**。首屏提交较修复前那一轮51.624397 ms更晚，因此不宣称整体打开更快，也不凭单次结果作稳定性承诺；M2新指标仍只记录基线，不套用输入或预载窗口门限。

结论：该单次样本支持同步首屏准备阻塞draw的问题已按设计修复，原始失败证据保留。六类三轮、既有原生输入 / 改宽 / 字体回归、IME与人工验收仍未补齐，恢复矩阵需另行确认并使用新目录。

## 修复后的正式宿主性能矩阵

来源 `.pi/tmp/m2-host-perf-after-layout/`，用户在正常宿主运行 `perf`；代理仅离线读取核对，没有打开原生窗口或重跑。42份CSV全部逐项复核既有预算和覆盖，四组summary均由CSV重算一致，M2最终磁盘字节与2000项补全目录再次独立验证。四组completed/restored=true，stderr全部为空，blank_frames=0。

环境：同Linux宿主、AMD Ryzen 7 7700 / RADV RAPHAEL_MENDOCINO、Mesa26.2.5-arch1.1、硬件渲染、release/native-profiler；fixture摘要与Noto Sans CJK SC regular/bold均与R1/M1及上次诊断一致。四组二进制/驱动摘要一致且匹配核对时文件，tracked patch与工作区一致，基点仍为abc0f88，包含未提交及untracked源码，不能把HEAD当作完整源码快照。正文864×1306、普通改宽644×1306，与历史838×1306 / 624×1306不同；M2 scratch启动869×1312、补全区展开时864×1266。按本轮实际尺寸复核原数值门限，不宣称严格同条件的性能改善，不改预算；本轮没有记录新的合成器版本。

- **M2六类各三轮：18份/558行**，每轮31步，含108次打开、126次切换、2000项补全/取消与三次重开循环。draw最大 **9.809 ms**；输入同步/首次/ready最大 **1.028 / 10.892 / 182.803 ms**，满足2/16/300 ms及draw16 ms门限。各轮最终仅scratch一个buffer，关闭view弱引用均不再存活。
- **普通原生六类各三轮：18份/1278行**。预载窗口冷打开首次/ready最大 **177.114 / 358.107 ms**；四类输入同步/首次/ready最大 **0.653 / 11.442 / 222.667 ms**；draw最大 **10.891 ms**。滚动/forced提交最大 **2.857 / 0.841 ms**；改宽/字体第一份场景提交最大 **16.349 ms**、完整新布局最大 **213.846 ms**；32键整批同步/首次/ready最大 **32.217 / 33.446 / 206.976 ms**。18次idle阶段均无draw，全部满足对应既有门限。
- **M1 mixed/single-ascii各三轮：6份/7380行**，含174步命令链及7200键持续输入，每轮60秒/50 ms。冷打开首次/ready最大 **173.749 / 381.158 ms**；命令链同步/首次/ready最大 **0.782 / 11.796 / 213.963 ms**；持续段同步/首次最大 **0.397 / 11.314 ms**，已观察ready最大 **172.407 ms**，各轮末键完整就绪。draw最大 **11.041 ms**。single-ascii每轮1181个旧目标ready被取代，共3543个保留未测；没有被取代的first，不填0冒充ready。调度迟到最高17.014 ms，仅报告而非实体按键延迟或新增门限；本轮不替代其他四类持续输入复测。

M2新指标仍只作基线，不套预载窗口或单键门限：完整visit-file首次 **24.236–70.462 ms**、ready **115.468–217.524 ms**；切换提交 **0.731–9.016 ms**；普通保存UI观察 **12.840–32.663 ms**，补全/取消观察 **5.389–20.825 / 5.088–16.100 ms**。save-edit中1轮在y输入前已观察到保存确认，17轮未观察到；后者不证明与实际worker磁盘I/O重叠。RSS最高390448 KiB，短循环内上升不能据此判定泄漏或无泄漏；view释放也不等于allocator/GPU缓存全部回收。

结论：修复后本轮正式perf的覆盖、功能断言与既有数值预算复核通过，不再处于仅单次诊断状态。旧失败与诊断产物全部保留；未重跑、未优化、未调阈值。离线核对索引 `.pi/tmp/m2-host-perf-review.json`；IME/manual尚未授权执行，不能宣布M2/M3整体验收通过。

## 正式宿主 IME 代表性复测

来源 `.pi/tmp/m2-host-ime/native-ime/`，用户另行同意后在正常宿主执行，代理仅离线核对，没有注键或重测。mixed / single-ascii各三轮，共6份/54行，覆盖正文初次预编辑/取消，以及滚动、改宽、换字体、点击后的第二次预编辑/提交；不是六类全量IME或完整minibuffer IME验收。

同上轮perf的Linux宿主、RADV/Mesa硬件路径、release/native-profiler、fixture、字体及二进制/驱动摘要，核对时文件匹配；正文864×1306、改宽644×1306。tracked patch与核对时工作区一致，仍不是完整源码快照；历史尺寸可比性限制同上。每份CSV顺序、预编辑/取消/提交的字节数变化、arm-to-submit边界、既有预算及stdout的ARMED/DONE/READY协议均通过，summary由CSV重算一致。completed/restored=true，stderr全部为空，blank_frames=0。

- mixed的IME首次/ready提交均为 **17.808–43.161 ms**；single-ascii首次 **17.971–34.964 ms**、ready **183.799–247.820 ms**，满足75/300 ms门限。arm耗时包含IPC、wtype、合成器、fcitx5和固定键间隔，不是纯IME回调或实体按键时延。
- 预载窗口冷打开首次/ready最大 **176.695 / 354.707 ms**；draw最大 **12.255 ms**；滚动首次最大 **1.683 ms**；改宽/字体第一份场景最大 **29.004 ms**、完整新布局最大 **189.132 ms**，均满足对应既有门限。
- 保留12张候选截图；抽查 `mixed-1.preedit-0.png` 与 `single-ascii-3.preedit-6.png`，预编辑可见、候选窗在caret附近。仅为这两个时点的视觉证据，不代替全部候选位置、焦点恢复与minibuffer的人工观察；截图也不提供屏幕呈现时延。

结论：本轮已覆盖的真实IME流程及既有数值预算复核通过，恢复状态由驱动检查，未调阈值、未优化或重测。离线核对索引 `.pi/tmp/m2-host-ime-review.json`。人工键盘文件闭环、完整正文/minibuffer IME观察与OS关闭仍未授权执行，M2/M3整体验收未完成。

## 正式宿主人工验收

来源 `.pi/tmp/m2-host-manual/`，用户另行同意后按验收文档操作两个正式窗口。`observations.tsv`五组结果均为yes，且与`files/result.json`一致：键盘文件闭环、正文IME、minibuffer IME、OS关闭取消/拒绝与保存后关闭。候选位置、取消后的正文/point/焦点、覆盖查询单键y、RET不默认同意以及OS关闭后的数据保留等，以用户按文档确认的观察为证，不冒充代理独立重现。

离线再次逐字节读取四个文件，保留原始result.json，没有重跑GUI或覆盖原报告：

- alpha.txt与overwrite.txt均为UTF-8 BOM + `X你alpha\r\nsecond`，20字节、CRLF、无末尾换行。
- beta.txt为`Ybeta\n`，6字节、LF及末尾换行。
- close.txt为`Zclose\n`，7字节，证明最终保存字节符合预期；中间取消/拒绝的数据保留依赖用户观察，不能仅从最终文件反推。

fixture种子摘要符合原配方；release构建与COMPLETED标记完整，两个窗口stdout/stderr均为空，脚本正常结束。tracked patch与核对时工作区一致，基点abc0f88；manual未单独采集运行时GPU/字体环境或二进制摘要，不据此推导硬件性能或完整源码快照。离线索引 `.pi/tmp/m2-host-manual-review.json`。本轮人工功能与磁盘字节核对通过，performance仍为unmeasured。

## 阶段最终结论

**M2与M3按既定v1范围验收通过。** 结论结合以下证据，而非仅看脚本退出码：

- 既有M1行为验收、M2功能/失败安全/保存快照与排队/会话隔离回归，以及本轮首屏修复后的默认Rust320、profiler341、Python37项回归、clippy/fmt等检查和独立headless六类各三轮性能证据；Emacs31.1的37项ERT与六个M-x pty用例仅作外部参照，不冒充产品GUI测试。
- 正式宿主perf的M2六类各三轮、普通原生六类各三轮与M1两类代表性持续输入，共42份报告，覆盖、功能断言和既有数值预算已复核。
- 另行授权的真实Pinyin IME两类各三轮，共6份报告；用户人工正文/minibuffer IME及键盘文件/OS关闭闭环，五组观察与四份最终文件精确字节均通过。

范围内F1按既有回归、Emacs参照与本轮真实观察覆盖核对，未发现待修复差异；F2/F3沿用 [已登记契约](../emacs-fidelity.md)，无新增发布豁免，不把延后功能纳入v1。本轮补测完成，未自动优化、调阈值或重跑，旧失败及诊断证据全部保留。完成状态见 [实施计划](../plan.md)。宿主报告复核阶段仅补充离线核对与文档，没有重跑桌面验收。产品修复、正式验收工具、回归测试及本记录一并提交；提交前本地检查见下节。

### 提交前检查与临时产物整理

提交前再次运行默认Rust320项、native-profiler341项、Python37项测试，全部通过；两组 `cargo clippy --all-targets -- -D warnings`（分别不带/带native-profiler）、`cargo fmt --check`、fish语法和diff检查通过。日志前缀 `.pi/tmp/m2-commit-`，没有重跑原生窗口或性能矩阵。

正式源码、工具、测试及文档统一收尾提交，原始宿主报告不改写。清理约232 MiB，范围仅为 `.pi/tmp/abs`、`.pi/tmp/m2_review_probes` 两个可再生成的临时二进制及 `.pi/tmp/python-cache/`；保留探针源码、验收及失败诊断的完整报告/样本、本地验证日志与离线核对索引。未清理 `target/` 构建缓存。

### 保留限制

- M2打开/保存/切换/补全新指标仅建立基线，未批准新门限；不套用预载冷窗口或单键预算，不宣称整体打开加速。既有门限在本轮实际正文尺寸下复核，尺寸与历史归档不同，不作严格同条件改善比较。
- 本轮原生持续输入与自动IME仅mixed/single-ascii代表性复测，不记其他四类本轮完成；过期目标ready仍保留未测。人工IME只覆盖文档中的流程，不声称穷举输入法、候选状态、键盘布局或平台；X11等未覆盖环境不记通过。
- CPU/平台提交、arm到提交与人工观察不提供GPU执行、实际屏幕呈现、纯实体按键/IME回调端到端时延。manual没有性能数据；本轮也没有新的合成器版本记录。
- 短周期RSS/view释放不证明长期无泄漏或GPU/allocator缓存全回收；保存与worker磁盘I/O是否真正重叠、持续加载中输入未测。文件缓存不受控，不宣称冷磁盘读取。
- 证据采集时绑定未提交的工作区及报告摘要；当时的tracked patch/HEAD不包含untracked源码完整快照，不把abc0f88当作本轮实现提交。后续提交包含这些正式源码，但不回填或改写原始宿主报告的版本记录。今后真实环境复测仍须另行取得用户同意并使用新目录；出现差异或超预算先报告，不自动调优重测。
