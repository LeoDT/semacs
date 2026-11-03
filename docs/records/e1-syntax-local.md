# E1 Rust tree-sitter：本地验证

## 身份与范围

- 本地环境：Linux x86_64，kernel `7.2.11-arch1-1`，Rust `1.99.1`；GPUI TestPlatform / TestTextSystem，不访问桌面。
- 源码基线：`c92cc320825050c102730aec62b21dfad2149f95` 加本轮 E1 未提交差异。`Cargo.toml` / `Cargo.lock` / `THIRD_PARTY.md` 及 `src/`、`scripts/`、`examples/`、`tests/`、`types/` 下 Git 跟踪与非忽略新增文件共 164 项，按路径排序的 SHA-256 清单摘要为 `193df7906f52514de59b852125d3c84a408d4f51c7c254f8b878dd120937bacd`；本地清单 `.pi/tmp/e1-source.sha256`。文档与临时产物不参与该源码清单，避免证据正文自引用。
- release 默认特性构建，无 `RUSTFLAGS`、`CARGO_ENCODED_RUSTFLAGS` 或交叉构建 target 覆盖；依赖按 Cargo.lock 固定。
- 用户批准范围见 [E1 行为契约](../emacs-fidelity.md#阶段-e1rust-tree-sitter)，执行入口见 [验收流程](../acceptance.md#宿主-fish-入口)。没有增加语言平台、缩进、LSP 或 JS 语言接口。

| release 产物 | SHA-256 |
|---|---|
| `semacs` | `05ab9c01e7757ad7d5f83f18d8232d28454f70567a9e2ba5a1a80adee3400ac8` |
| `examples/perf_view` | `c91b9726f34c60cf2df4ebeeb9c1bed3bbc4f65e5214cd4a3b20b5b91d10350c` |
| `examples/perf_m1` | `46c3bf889e79350f16e33258d415dcf9a314391c1d6cb6fff3ba086c5ca78d24` |
| `examples/perf_syntax` | `4f529d2ce34716f09a0c99daa9855c4e5055128c210ed50296572ebc3e2691d0` |

## 本地检查

- `cargo test --all-targets`：470 项通过，无失败 / 忽略；含新语法基线小样本 workload 自检。
- 默认与 `native-profiler` 的 `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` 均通过。
- `python3 -B -m unittest discover -s tests -p 'test_*.py'`：50 项通过；manual fixture 工具离线测试：16 项通过；`fish --no-execute scripts/acceptance.fish` 通过。
- `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts` 通过。
- 正式 release `semacs` 与三个 headless example 构建通过。没有运行真实窗口、用户配置或工作文件。

覆盖包含：合并编辑 / Unicode 字节坐标与 fresh parse 对照，32 个固定种子的编辑序列、undo / redo、错误与未完成语法；过期文本与语言代次、单文档有界任务和 observer / document 释放；分屏共享同一个高亮结果；另存成功 / 失败的身份与语言变化；预编辑只投影已提交颜色；prompt / 光标优先级、超出 inline 容量的装饰不截断，以及高亮 / 换色不改变 shaping、scroll 或输入几何。配置覆盖全部 22 个样式，并拒绝未知语法样式及语法背景属性。

测试与静态检查日志：`.pi/tmp/e1-tests-post-review.log`、`e1-clippy-post-review.log`、`e1-clippy-feature.log`、`e1-python-tests.log`、`e1-manual-tools-tests-final.log`、`e1-release-post-review.log`。

## 集中审查与工具修正

阶段实现及初次自检后集中 reviewer 一次，发现一个 P2：取消时已完成、尚未被前台接收的 task 输出可能在前台释放大语法树。修复以统一后台所有权保护覆盖已安装结果、待运行任务的输入和已完成任务的输出；确定性回归只执行后台 lane 使结果完成，再在前台取消接收，验证树与探针最终在 TestDispatcher 的后台 lane 析构。此项不是原生线程时延测量。主代理完成修复与完整自检，没有追加 reviewer。

新 `perf_syntax` 首轮误把 redo 后 point 当成编辑末端，触发大文本断言；当时新工具未沿用既有 TestDispatcher 退出清理，还出现超时。只修正新工具：按既有历史契约在编辑起点构造 burst 预期、限制失败输出，并沿用既有调度器清理；未修改产品 redo 或放宽断言。原始失败 `.pi/tmp/e1-perf-syntax.log`、`e1-probe-selftest.log` 保留，不记为通过。修正后的小样本完整 workload 自检及新批次通过；最终性能证据采用下述审查修复后的批次。

## 既有 headless 性能回归

`perf_view` / `perf_m1`：既有六类约 8 MiB 普通文本，各三轮；初始 960×640，改宽 720×640，默认 monospace 16px / 24px，字体阶段 bold 18px / 27px，软换行开启。没有为普通文本启用 Rust。`perf_view` 共 18 行，`perf_m1` 为 522 个命令链步骤与 18 段未等待输入；实际指标按既有定义解释。

| 指标 | 本批最小–最大（ms） |
|---|---|
| 冷首帧 | 0.383–0.608 |
| 初始完整布局 ready | 96.259–155.808 |
| 改宽 / 字体完整布局 ready | 96.018–155.778 / 96.023–156.410 |
| 滚动 | 0.136–0.222 |
| 稳定 forced / empty 均值 | 0.048–0.071 / 0.047–0.071 |
| 四类编辑 front（合并范围） | 0.236–0.383 |
| 四类编辑 ready 最大值 | 169.487 |
| M1 命令链 action / front / ready 最大值 | 0.445 / 0.591 / 162.584 |
| M1 未等待输入段 action / front / 最终恢复最大值 | 0.398 / 0.585 / 0.353 |

已覆盖指标满足既有 headless 预算；M1 的整段输入及末键恢复不冒充长时间持续输入或逐键 ready。正文模型构造另测为 1.570–5.437 ms，不包含在冷首帧定义内。原始报告为 `.pi/tmp/e1-perf-view-02.log`、`e1-perf-m1-02.log`，样本 `.pi/tmp/e1-perf-sample.txt`。

## 新语法基线

`perf_syntax`：960×640、默认字体与软换行；多行 Rust 8,388,690 bytes 与单行长 raw string 8,388,638 bytes，各三轮。冷启动不含 buffer 构造；`syntax_and_layout_ready_ms` 包含后台解析、查询及布局全部完成，不能当成旧纯布局 ready。没有为此新增数值门限，也没有套用或放宽旧预算。

| 样本 / 动作 | 同步分发（ms） | 首次 draw（ms） | 语法 + 布局就绪（ms） |
|---|---|---|---|
| Rust 冷窗口 | 未单列 | 0.461–0.577 | 1499.822–1640.449 |
| Rust 插入 | 0.289–0.297 | 0.367–0.376 | 986.506–1009.080 |
| Rust undo / redo（合并范围） | 0.204–0.215 | 0.285–0.297 | 923.410–929.365 |
| Rust 同回调 32 键 | 5.023–5.092 | 5.099–5.169 | 1854.279–1860.728 |
| Rust 换色 | 0.172–0.183 | 0.255–0.268 | 0.335–0.348 |
| 长 raw string 冷窗口 | 未单列 | 0.603–0.623 | 142.434–142.839 |
| 长 raw string 插入 / undo / redo（合并范围） | 0.307–0.446 | 0.414–0.555 | 120.261–120.918 |
| 长 raw string 同回调 32 键 | 9.527–9.561 | 9.632–9.666 | 129.519–129.642 |
| 长 raw string 换色 | 0.276–0.279 | 0.381–0.383 | 0.482–0.485 |

每轮 100 次强制重绘均值：Rust 0.075–0.076 ms，长 raw string 0.097 ms。基线断言当前文本版本的高亮 / 完整布局可用、正文可见、编辑后精确文本和换色不改变 snapshot。多行 Rust 的完整着色明显晚于首帧，作为本批新基线如实保留，不把异步普通文本首帧记作语法已就绪。原始报告 `.pi/tmp/e1-perf-syntax-03.log`。

## 结论与未测边界

E1 已批准范围的本地实现、回归、一次审查后的修复及 headless 检查通过。`syntax` 宿主模式已通过离线工具自检，但未启动；后续须先取得本轮同意，再定向确认真实字体下的颜色、分屏、历史、另存和布局 / 保存效果。

尚未测原生性能、真实 IME、GPU / 实际呈现，不以 headless 回归、单键前台成本或新语法就绪基线外推其通过。旧 M2 profiler 生命周期失败仍按计划延期至 E1 后处理：本批只确认 feature Clippy 可编译，不宣称完整 feature 测试通过或旧工具已修复。
