# E2 宿主反馈修复：本地验证

## 身份与范围

- 基线 `8e680a0d78b35634ad278d6ff27664dd2b054371` 加未提交 E2 差异及本轮修复；前一宿主失败身份见 [首批回传](e2-languages-host-01.md)，原始目录及报告未修改。
- Linux x86_64 / `7.2.11-arch1-1`，Rust `1.99.1`；GPUI TestPlatform / TestTextSystem，不访问桌面。release 默认特性；原生 probe 单独启用 `native-profiler`。
- 按路径排序的 `Cargo.toml`、`Cargo.lock`、`THIRD_PARTY.md` 及 `src/`、`tests/`、`examples/`、`scripts/`、`types/` 下 Git 跟踪与非忽略新增文件共 175 项，SHA-256 清单摘要 `61ebc85156f90370786bb0fa25f045a0ea84c374a0600d231c1825cb53378950`；本地清单 `.pi/tmp/e2-fix-source.sha256`。文档及临时产物不参与源码清单。
- 用户已批准修正 RET 工具及编辑期间临时配色策略；没有批准新的真实环境批次。本轮不改变 newline、输入策略、身份断言或性能预算。

| release 产物 | SHA-256 |
|---|---|
| `semacs` | `b1237cdcf685ee9ca7f3958be9b3035b5432d51304e8ae92601bcbb3e3e54643` |
| `examples/perf_native`（profiler） | `ec17dbfb480b8e35d943d0e7378d34c8cde5520fb504012b6627c0d9ff6cce4b` |
| `examples/perf_view` | `da5b5392cb4e1bc832caf0d3d9450f1eb66b25b2151fb5031aec1f63805ab6a6` |
| `examples/perf_syntax` | `b69e7b920d237fb77f8bec4c2c359eca1fa2c96be686ed0107997e29436d9b3b` |

## 修复与检查

- RET 的独立预期按既有空白清理契约修正：newline 用 LF 替换输入点前的空格，Join 删除 LF 后不恢复空格；最终正文为原 fixture 删除该空格，point 同样减少一字节。保留逐阶段全文 / point 检查及独立最终断言，Rust 与 Python 同步核对每个阶段 bytes / point。错误信息区分缺失 paint 与已有 paint 但目标不符。
- 将临时绘制配色与当前快照解析结果分离。编辑映射保留未改动字节，插入 / 替换内容普通色；最终结果只在当前快照及语言代次有效时更新显示。当前行为见 [语法契约](../emacs-fidelity.md#阶段-e1rust-tree-sitter)，不改写旧验收记录。
- 临时映射使用持久化区间树，编辑只拆分边界并共享未改部分；不逐键复制全文 spans。worker 移走解析 base 时临时配色仍可用，绘制缓存不会使 `syntax_snapshot()` / `syntax_ready()` 提前报告完成；废弃映射和最后的高亮所有权在后台释放。
- `cargo test --all-targets` 493 项、`--features native-profiler` 533 项全部通过，无忽略；两套 Clippy、`cargo fmt --check`、`git diff --check` 通过。
- Python 68 项、人工 fixture 工具 23 项及 fish 语法检查通过；release 应用、两个 headless example 及 release profiler probe 构建通过。
- 新增回归包括 Markdown / HTML 约 256 KiB fixture 的完整实际按键序列，错误 RET 旧预期拒绝检查，32 个固定种子各 128 次 Unicode 编辑的独立逐字节配色 oracle、裁剪 / 预编辑、清空后插入，以及 pending 连续编辑 / 历史 / 分屏共享 / 语言切换 / 失败恢复 / 后台释放。
- 实现及自检后集中 reviewer 一次，无实质问题或待修项；独立复跑 6 项 Rust、17 项 Python 测试通过。未追加审查或真实环境运行。

全量及工具日志为 `.pi/tmp/e2-fix-test-default.log`、`e2-fix-test-profiler.log`、`e2-fix-clippy-default.log`、`e2-fix-clippy-profiler.log`、`e2-fix-test-python.log`、`e2-fix-test-manual.log`；构建日志同目录 `e2-fix-build-*.log`。

## Headless 回归

`perf_view`：六类约 8 MiB 普通文本，各三轮，初始 960×640、改宽 720×640，默认 monospace 16px / 24px，字体阶段 bold 18px / 27px，软换行开启。18 行逐项核对全部满足既有预算。

| 指标 | 最小–最大（ms） |
|---|---|
| 冷首帧 | 0.397–0.811 |
| 初始完整布局 ready | 95.337–152.680 |
| 改宽 / 字体完整布局 ready | 94.814–151.888 / 94.894–153.857 |
| 滚动 | 0.141–0.227 |
| 稳定 forced / empty 均值 | 0.044–0.062 / 0.044–0.061 |
| 四类编辑 front（合并范围） | 0.244–0.618 |
| 四类编辑 ready 最大值 | 164.704 |

`perf_syntax`：同为 960×640、默认字体、软换行，五类各三轮；语法 + 布局 ready 保持独立基线，不套用旧纯布局 300 ms 门限。

| 样本 / 字节数 | 冷首次 draw（ms） | 冷语法 + 布局 ready（ms） | 插入 / undo / redo ready（合并范围，ms） | 同回调 32 键 ready（ms） |
|---|---|---|---|---|
| Rust / 8,388,690 | 0.465–0.548 | 1541.138–1680.398 | 973.727–1064.261 | 1960.793–1979.847 |
| 长 raw string / 8,388,638 | 0.605–0.751 | 140.814–142.538 | 118.841–120.173 | 128.593–129.018 |
| TSX / 262,150 | 0.341–0.350 | 57.438–72.210 | 36.862–38.162 | 79.097–79.539 |
| HTML / 262,238 | 0.502–0.520 | 72.873–78.463 | 49.586–50.708 | 105.812–107.720 |
| Markdown / 262,256 | 0.350–0.412 | 161.619–168.159 | 147.383–162.313 | 291.130–303.382 |

语法样本的单次编辑同步动作最大 0.470 ms、front 最大 0.600 ms；同回调 32 键同步动作最大 9.767 ms。每轮 100 次稳定 forced draw 均值为 0.063–0.127 ms，换色 ready 为 0.279–0.551 ms。上述语法指标仅作同工具回归观察，不将批量输入当单键或原生性能验收；Markdown 一轮语法 ready 超过 300 ms 不构成已批准纯布局预算超限。

原报告 `.pi/tmp/e2-fix-perf-view.log`、`e2-fix-perf-syntax.log`；普通文本 fixture `.pi/tmp/e2-fix-perf-sample.txt`。未复跑不受影响的 M1 全命令矩阵。

## 结论与边界

两项修复、本地回归及集中审查完成，无已知待修项。自动测试支持临时显示与解析状态隔离及坐标正确性，不能替代真实字体下的闪烁目视确认；修正后的原生工具也尚未取得新宿主数据。

首批宿主原始停止结论保持不变，没有继续或重跑旧批次。下一步是按最小范围另行征求宿主复核授权及用户阶段收尾确认，不直接进入 F。真实 IME 时延、GPU / 实际呈现和未覆盖的大文件矩阵继续保留未测，不自动追加。
