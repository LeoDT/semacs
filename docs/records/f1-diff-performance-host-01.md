# F1 diff 原生性能首批：首帧工具启动失败

## 批次与身份

用户明确批准 small 三轮、每轮一个顺序窗口及内部左右分屏，测试 staged / unstaged 打开、折叠 / 导航 / 长行可见、滚动、刷新与 idle；合成 GPUI 动作，不注入 OS 按键、不切换输入法、不截图。用户在正常 fish / Wayland 宿主运行 `diff-perf` 后回传 STOP。

原始目录 `.pi/tmp/f1-diff-perf-host-01` 保留，没有修复或覆盖 fixture，没有启动新窗口或重试。内层 `diff/` 有构建日志、源码身份、环境、冻结 small 仓库及 stdout / stderr，没有 CSV / spans 或 summary。

- HEAD：`d6a7ba4890f30e24b70794d78294535d9239a440`，工作区未提交改动由内层相对 HEAD 的完整补丁及 untracked 内容归档。
- 内层源码补丁 SHA-256：`9d896e83366172ee70e4e8189c41509e8db0ce1622d8c09c7c90cf4bae2d33d1`
- 受测 `perf_native` SHA-256：`bb141174a835a22abf317587394a4cc3ea35e528b4538e9949650c196b7e81f0`
- 驱动 SHA-256：`9c5a5e58bca4163f3a69c9bab6b6740d109f556572cc024a87bea9724b87f6a4`
- small manifest SHA-256：`55614e5f88ed52f27defe076ae0554f900a04771575bf42b3b88c61ff2542f5e`

离线核对源码归档及 HEAD / status 摘要、驱动和修复前的实际 release 二进制身份一致；重新检查 fixture / HEAD / index / Git 配置 / oracle 与冻结 patch、空配置通过。修复前产品 / 工具源码相对宿主归档仅有后续计划文档差异，没有把其他版本当成受测版本。

## 实际结果与根因

`small-1.stderr.log`：

```text
thread 'main' (...) panicked at examples/support/native_diff.rs:29:44:
called `Option::unwrap()` on a `None` value
```

首窗进程 exit 101，stdout 为空，没有 `GRAPHICS`、`STAGE` 或任何性能结果；后续两轮未启动。fish 和 Python 均按失败停止并保存 STOP。宿主为 Linux x86_64 / Wayland，DISPLAY `:0`、WAYLAND_DISPLAY `wayland-1`；尚未取得 renderer specs、实际正文尺寸或计时证据，不能将硬件入口预检解释成实际 GPU 已验证。

根因是 probe canvas 通过 `Rc<RefCell<Option<Run>>>` 读取绘制目标，而 Run 要在 `open_window` 返回后的 update 才创建。GPUI 在窗口打开过程中已触发首次 render / paint，canvas 提前 unwrap 了 None。此前共用 headless 工作负载测试只绘制 AppShell，没有覆盖实际原生 Probe 的初始化次序，因此未捕获此问题。

这是性能工具启动失败，**不是预算超限，也不是任何 diff 性能阶段已完成**；不能从该批次判断产品性能或 GPU / 呈现行为。

## 本地修复与回归

原生 Probe 现在直接持有已挂载 shell 和共享 view 列表，首帧观测在构造 Probe 时即可独立工作；Run 在打开窗口后完整构造，调度器不再承载 Option 初始化状态。动作 / buffer 激活后同步 view 列表，既保留首帧观测，也继续按同一场景验证双 view，不用跳过首帧或空值容错掩盖缺失。

新增回归实际绘制 Probe，先于任何 Workload / Run 创建取得首帧，再通过真实 Run.begin 分屏 / 选择 / 取消分屏，核对挂载 view、选择身份、可见正文及兼容布局。测试使用 TestPlatform，不启动宿主窗口。

完整默认与 profiler Rust 自检、两套全目标 Clippy、Python 95 项、fmt / diff / fish 检查及 release 原生 example 构建通过；详细日志索引为 `.pi/tmp/f1-diff-startup-{default,feature}-tests.log`、对应 `-clippy.log`、`-python.log`、`-release.log` 和定向首帧回归日志。没有改动产品 diff 链路、共用工作负载、驱动或数值预算，不机械重跑已有 headless 性能矩阵；此局部修复由主代理自检，没有追加 reviewer。

本地回归只证明修复结构与实际 Probe 的离线绘制路径；**修复后的原生启动与性能仍须新批次确认**。下一步向用户请求原 small 三轮范围的新授权，使用新目录 `.pi/tmp/f1-diff-perf-host-02`，不自动重试、不覆盖本批失败证据。
