# E3 首版排版性能阻塞（未验收）

## 身份与范围

- 基于 `138995b31a615833e04a729e8d619c829b9667c4` 的 E3 工作区首版，尚未提交，也不是阶段交付。
- 测量前的产品 / 测试差异加两个新增 Rust 文件，经 `cargo fmt` 格式化后的合并差异 SHA-256 为 `e6117b0b9df556f645153ac222a2820226ac89033da4d7cfb043c62b344e4558`；格式化不改变被测逻辑。合并方式：`git diff -- src tests`，随后依次追加 `git diff --no-index /dev/null src/buffer/annotations.rs`、`git diff --no-index /dev/null src/display/typography.rs` 的输出。
- 本次接入主题字体 / 装饰属性、Markdown 独立语义和强调合成、带样式字体 run、范围样式及行背景原语，并在现有有界扫描中使用完整候选行 shaping 校验换行边界。首版没有完成 E3 专门回归、性能修复及集中审查，不据此认定功能全部正确。

## 本地结果

- `cargo test --all-targets --quiet` 通过；包括 502 项 lib / 集成测试及 example 内测试。主要是既有回归，并按已批准新角色 / 属性调整旧断言，不能替代尚未补齐的 E3 新功能测试。
- `cargo build --release --example perf_view --example perf_m1 --example perf_syntax` 通过。
- `cargo fmt` 及 `git diff --check` 完成。Clippy、完整 profiler feature、类型声明校验及新功能专门测试尚未完成。
- 未运行桌面窗口、真实字体 / fallback、真实 IME、原生性能或 GPU 测量。

## 首批 headless 性能失败

使用 `python3 -B scripts/perf_sample.py .pi/tmp/e3-sample.txt` 生成 8,388,648 字节 mixed 样本，release `perf_view` 使用 TestPlatform / TestTextSystem、960×640 初始窗口及既有字体 / 改宽配置。

实际调用为 `RUNS=3 SHAPES=mixed target/release/examples/perf_view .pi/tmp/e3-sample.txt`。此 example 不读取 `SHAPES`，因此实际输出全部六类、每类三轮；它仅输出 CSV，不在内部执行预算停止。该限制是在本批返回并检查 CSV 后确认的，不能声称本批在第一个超限样本处自动停止。发现失败后没有运行 `perf_m1`、`perf_syntax` 或其他后续性能批次，也没有调优重测。

| 样本 | 初始完整布局 ready（三轮范围，ms） | 对照 300 ms 预算 |
|---|---:|---|
| mixed | 506.050–586.301 | 失败 |
| short-ascii | 534.137–545.205 | 失败 |
| short-unicode | 225.940–228.663 | 本项满足 |
| long-ascii | 570.371–576.022 | 失败 |
| single-ascii | 566.141–584.714 | 失败 |
| single-unicode | 333.564–341.352 | 失败 |

第一条 mixed ready 即为 586.301 ms。多个样本的改宽 / 字体 ready 同样超过 300 ms；single-ascii 的输入 ready 约 601–612 ms，single-unicode 约 352–356 ms，也不满足既有输入 ready 预算。本批不能报告布局或输入性能通过。

原始构建、测试及性能输出保留于 `.pi/tmp/e3-build.log`、`.pi/tmp/e3-tests.log`、`.pi/tmp/e3-perf-view.log`，未覆盖或删除。这里保留可追溯摘要，不将全部临时日志入库。

## 结论与下一步

E3 保持实施中且性能阻塞，不收尾、不提交、不进入 F。当前候选行校验路径增加了 shaping、字体 run 构建及分配工作，是下一步定向分析对象，但尚未用 profile 证明各项成本占比；不把推测当作已定位根因。

按验收停止规则先向用户报告。待用户确认继续后，先定向分析排版热路径，保留真实字体 shaping 一致性，不以回退到无样式测量、固定宽度假设、关闭换行或放宽预算换取通过；修复后以新批次验证。后续批次须在外层逐样本检查预算并停止，不能再次误用 `SHAPES` 为 `perf_view` 的过滤参数。
