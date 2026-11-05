# F1 diff 完整原生诊断：本地工具验证

## 已确认范围

第二批宿主在 setup CPU draw >16 ms、尚无 diff 动作数据时停止，见 [失败记录](f1-diff-performance-host-02.md)。用户指出不应让提前超限遮住后续问题，并明确确认切换原 small 样本三轮完整诊断：性能超限记录后继续全部动作和本批后续窗口；正文 / 投影 / 几何 / 身份错误、异常及安全超时仍停止；门限不变，报告是诊断、非验收。

本轮只实现和离线验证工具，没有启动新宿主窗口，没有运行新的 headless 性能矩阵，也没有改写前两批目录或既有基线。用户已同意的三轮诊断由其正常 fish / Wayland 终端执行，拟用新目录 `.pi/tmp/f1-diff-diagnostic-host-01`，入口为 `diff-diagnostic`，不是原 `diff-perf`。

## 工具边界

- 同一生产命令工作负载仍为每窗 staged / unstaged 各 13 阶段，共 26 阶段，三窗共 78 行；不增加 OS 注键、IME、截图、改宽 / 字体或其他样本。
- 显式 `--diagnostic` / fish `diff-diagnostic` 才允许性能超限继续；原严格入口保持超限停止。新模式覆盖 setup CPU draw、动作 draw、垂直滚动首次提交和 idle draw，数值门限全部保留。
- 窗口启动、命令准备、动作前帧及测量动作全部记录逐帧侧表：draw / request-layout / prepaint / paint / other、CPU submit、提交终点及动作起点、选择 view、真实 display / 正文及折叠投影、selected / all-view 兼容与 ready 证据。setup 不冒充动作时延；尚未 ready 与不兼容身份分开，后者即使 boot 已完成也硬停止。
- 动作与原生逐帧超限写出具体数值和阶段、标为 `over-budget`，报告及环境明确 `diagnostic=true; acceptance=false`。总体预算判定包含 setup；`completed=true` 仅表示采集完成，不能解释为预算通过。即使全部数字合格，普通读取入口也拒绝把诊断报告当验收。
- idle 产生 draw 是预算失败，可采满 30 tick 后继续，但不造出 idle 动作时延。正文、投影、同场景共享 snapshot、选择 view 或观测身份不满足仍硬停。
- 父驱动按窗口打开、阶段准备和动作的明确进度更新 35 秒 watchdog；子进程 setup / 动作 30 秒观察上限保持。两者是资源保护，不是性能预算；没有目标提交不补零、没有就绪不强行推进，也不因异常而自动重试。
- CSV 读取核对逐帧时间与阶段顺序、动作帧数 / 最大 draw、完成时间对应的真实提交及其目标正文版本 / 字节 / 折叠投影 / point 和 selected / all-view ready。不能仅因某帧提交发生就认作目标就绪，也不要求 pending 帧提前是最终身份。
- 每轮后保存已验证的累计 summary 和结果；纯性能失败继续下一窗，硬失败保留前面有效结果与当前部分日志，不推进剩余窗。逐帧和动作超限条目可能描述同一事件，不相加作为成本或独立故障数。

CPU submit / draw 仍不证明 GPU 执行或屏幕呈现；嵌套 preparation spans 不相加。正式行为和预算见 [性能文档](../performance.md) 与 [专项验收](../acceptance.md#f1-diff-专项性能)，不在本记录另设门限。

## 本地验证与审查

HEAD 仍为 `d6a7ba4`，F1 与工具未提交。最终本地 release 身份：

- `perf_native` SHA-256：`f8ffa4e7a45e048b89e354af2b6a2c36efc85e1dcdf9695fedea2c529c065488`
- `scripts/perf_diff.py` SHA-256：`28a66ab6f88486e8db196b4181280105dc89e10c8f66873d728e7110e3f2b536`

本诊断行为阶段进行一次集中 reviewer；两项 P2 已修复，未追加审查：setup 坏身份 / 投影不能随 boot=true 被忽略；逐帧读取须证明目标 ready 的提交，而非只验证时间戳存在。修复及后续自检由主代理完成。

最终通过且没有跳过：

- 默认全目标 Rust **560 项**，profiler 全目标 **631 项**（native example **62 项**）；两套全目标 Clippy。
- `tests/test_*.py` **103 项**，其中 diff 驱动 / fixture / 报告 **22 项**；fmt / diff / fish 检查和 release 两个 example 构建通过。
- TestPlatform 实际 Probe / 命令路由的控制器回归模拟慢 setup、慢 draw / 滚动及 idle draw，26 阶段全部完成并保留预算失败，末尾到达 unstaged unsplit。生成的实际 CSV 再交 Python 逐帧读取器交叉验证，仍是合成 journal 时间、非性能实测。
- 严格模式慢 setup 停止；诊断中空白 setup、boot 后 command-prepare / between-actions 坏投影、比较身份错误、setup / 动作超时仍停止。合法 compatible-but-not-ready setup 不被误作坏身份。
- Python 回归覆盖三轮纯预算失败仍完成、后续硬失败停止并保留先前 summary、诊断非验收隔离、setup / idle 超限总体判定，以及旧 refresh 版本、无 ready 帧、帧组乱序、缺失 / 错误预算标记等伪证据拒绝。

日志索引：`.pi/tmp/f1-diff-diagnostic-final-{default,feature}-tests.log`、对应 `-clippy.log`、`-python.log`、`-release.log`，以及定向 `-protocol-test.log` / `-review-*.log`。临时测试仓库只在 `.pi/tmp/`，不操作日常 Git / 配置。

## 结论与未测

工具的本地完整继续路径、硬停止与报告交叉验证通过；不把离线合成 journal 时间当成原生测量。真实平台 `schedule → tick → collect_paints` 的首帧配对、提交分类、idle 和最终完整数据仍待本次获准的宿主运行确认；尚无修复后 diff 原生性能结论，也没有产品性能优化或数值预算变更。
