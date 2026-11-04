# E2 多语言语法：本地验证

## 身份与范围

- 环境：Linux x86_64，kernel `7.2.11-arch1-1`，Rust `1.99.1`；GPUI TestPlatform / TestTextSystem，不访问桌面。
- 源码：基线 `8e680a0d78b35634ad278d6ff27664dd2b054371` 加本轮 E2 未提交差异。按路径排序的 `Cargo.toml`、`Cargo.lock`、`THIRD_PARTY.md` 及 `src/`、`tests/`、`examples/`、`scripts/`、`types/` 下 Git 跟踪与非忽略新增文件共 168 项，SHA-256 清单摘要为 `5e2e5bf8efe92e7f285f27dc6a4b3dfb115d58b9608c90a84f2230037eb96044`，本地清单 `.pi/tmp/e2-source.sha256`。文档与临时产物不参与源码清单。
- release 默认特性，无 `RUSTFLAGS`、`CARGO_ENCODED_RUSTFLAGS` 或 `CARGO_BUILD_TARGET` 覆盖。依赖按 Cargo.lock 固定；来源和许可证见 [THIRD_PARTY.md](../../THIRD_PARTY.md)。
- 批准行为见 [E2 契约](../emacs-fidelity.md#阶段-e2多语言-tree-sitter)，技术边界见 [D27](../decisions.md#d27-内置语言与通用语法机制)。HTML 上游具有 script / style 注入范围，依用户条件确认一起接入；未扩大到全部 injections、动态语言加载或 LSP。

| release 产物 | SHA-256 |
|---|---|
| `semacs` | `1ae2a432862364539035b4bf791a137c3d5309dbdeaf07a7a9b94df8c2c28f83` |
| `examples/perf_view` | `48adf24af7670030fe8c06d3b295f3ceb73ede5a0f7b2efbc15bd955a63f6705` |
| `examples/perf_m1` | `be90bfd5771722b6db9bc3e0b6bf01390e1386460b8e29180ca93e12f74f774b` |
| `examples/perf_syntax` | `7f76e2edf68a4d96b01c81535276e32d6bc0e515288c6dabf8b528c2be6732b4` |

## 检查与审查

- `cargo test --all-targets`：488 项通过；`--features native-profiler`：519 项通过，均无失败或忽略。
- 默认 / profiler 两套 Clippy（`--all-targets -- -D warnings`）、`cargo fmt --check`、`git diff --check` 通过；正式 release 应用与三个 headless example 构建通过。
- 34 项语法相关筛选测试通过。覆盖 grammar ABI、全部组合查询编译和 capture 映射，各语言代表性语法、JSX / TSX、模板表达式，Markdown 块 / 行内 / GFM 表格、已知及未知围栏、HTML script / style、多层范围和引用续行标记。
- 增量结果与全量解析对照包含固定种子的多语言合并编辑、Unicode、结构破坏 / 恢复与 undo / redo；独立验证根树及嵌入树复用未变子树。另有 16,384 树边界退化、禁止进一步围栏递归、解析 / 查询 / 注入 / 合并的取消检查。
- 异步 / 集成覆盖语言代次取消、同语言别名路径保留结果、实际成功与失败另存、共享 view、预编辑隔离、换色不改变可用 shaping、历史与后台释放。不将 TestPlatform 的预编辑投影测试记作真实 IME。
- 实现及自检后集中 reviewer 一次，未发现待修问题或必须补齐的测试缺口；审查独立重跑上述 34 项筛选测试。主代理随后完成最终全量检查，未追加 reviewer。

最终日志：`.pi/tmp/e2-final-tests.log`、`e2-final-feature-tests.log`、`e2-final-clippy.log`、`e2-final-feature-clippy.log`、`e2-final-release.log`。

## 既有 headless 回归

最终版本的 `perf_view` / `perf_m1`：六类约 8 MiB 普通文本，各三轮；初始 960×640，改宽 720×640，默认 monospace 16px / 24px，字体阶段 bold 18px / 27px，软换行开启。普通文本不启用语法。共 18 行 view 测量、522 个 M1 命令链步骤与 18 段未等待输入。

| 指标 | 最小–最大（ms） |
|---|---|
| 冷首帧 | 0.407–0.693 |
| 初始完整布局 ready | 96.487–172.059 |
| 改宽 / 字体完整布局 ready | 96.483–173.315 / 96.470–167.895 |
| 滚动 | 0.142–0.231 |
| 稳定 forced / empty 均值 | 0.044–0.062 / 0.043–0.061 |
| 四类编辑 front（合并范围） | 0.245–0.776 |
| 四类编辑 ready 最大值 | 180.836 |
| M1 命令链 action / front / ready 最大值 | 0.788 / 0.900 / 163.320 |
| M1 未等待输入段 action / front / 最终恢复最大值 | 0.661 / 0.850 / 0.379 |

逐项核对已覆盖指标满足既有预算；未等待输入段不冒充持续输入或逐键 ready。原始报告 `.pi/tmp/e2-final-perf-view.log`、`e2-final-perf-m1.log`，样本 `.pi/tmp/e2-perf-sample.txt`。

## 语法与多层解析基线

`perf_syntax`：同为 960×640、默认字体、软换行，每种样本三轮。Rust / 长 raw string 延续 E1 的约 8 MiB 样本；TSX / HTML / Markdown 新样本各约 256 KiB，不能据此声称新增语言已做 8 MiB 全矩阵。Markdown 样本包含 HTML 围栏及其 JS / CSS 子层，未触及树数量上限。

| 样本 / 字节数 | 冷首次 draw（ms） | 冷语法 + 布局 ready（ms） | 插入 / undo / redo ready（合并范围，ms） | 同回调 32 键 ready（ms） |
|---|---|---|---|---|
| Rust / 8,388,690 | 0.461–0.638 | 1544.736–1816.302 | 954.239–1043.942 | 1927.947–1941.660 |
| 长 raw string / 8,388,638 | 0.604–0.965 | 145.342–155.957 | 122.337–124.007 | 132.046–132.501 |
| TSX / 262,150 | 0.342–0.531 | 56.851–68.551 | 35.844–37.115 | 76.324–76.871 |
| HTML / 262,238 | 0.502–0.512 | 71.804–76.728 | 46.847–50.741 | 102.038–103.513 |
| Markdown / 262,256 | 0.308–0.392 | 160.328–162.451 | 140.560–158.621 | 278.616–280.357 |

单次编辑 front 最大 0.616 ms；同回调 32 键为独立批量基线，不套用单键预算。每轮 100 次稳定 forced draw 均值：Rust 0.072–0.074 ms，长 raw string 0.094–0.098 ms，TSX 0.070 ms，HTML 0.113–0.114 ms，Markdown 0.059–0.060 ms。换色的完整 ready 为 0.268–0.462 ms，快照保持不变。

`syntax_and_layout_ready_ms` 包含解析、查询、子语言和布局；Rust 完整着色仍为秒级，不套用旧纯布局 300 ms 门限，不把普通色首帧记成语法就绪，也未新增或放宽预算。基线工具断言当前文本版本高亮与完整布局可用、正文可见、历史 / burst 后精确文本和换色快照。

首轮新基线发现无 injections 的语言仍额外遍历整棵语法树。移除空查询扫描后执行新批次；原日志 `.pi/tmp/e2-perf-syntax.log` 保留，不覆盖。上表采用 `.pi/tmp/e2-perf-syntax-02.log`；最终 release 重建后 SHA-256 与该批二进制一致。

## 结论与未测边界

E2 批准范围的实现、本地回归、集中审查与 headless 检查已完成，无剩余已知必修项。是否进行新增语言的真实字体 / 配色目视观察及阶段收尾，待用户确认；未自动启动 F。

没有访问桌面、读取用户真实配置或操作日常文件。新增语言的大文件全矩阵、原生性能、真实 IME、GPU / 实际呈现及真实字体下的新增配色组合未测，不以自动化或代表样本外推其通过；旧阶段已接受的未测边界不自动产生补测任务。
