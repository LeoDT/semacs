# M2 profiler 生命周期工具：本地修复验证

## 身份与范围

- 用户要求在 E1 后处理旧 profiler 失败；本批只修工具期望与报告校验，不改变产品 buffer 生命周期、D1 消息历史或已有性能预算。E1 本地 / 宿主记录保持冻结。
- Linux x86_64、Rust 1.99.1，GPUI TestPlatform / TestTextSystem；未访问真实桌面。
- 基线 `c92cc320825050c102730aec62b21dfad2149f95` 加 E1 及本次工具修改。对 Cargo 文件、THIRD_PARTY.md、`src/`、`scripts/`、`examples/`、`tests/`、`types/` 下 164 个 Git 跟踪 / 非忽略新增源码文件按路径排序的 SHA-256 清单，摘要为 `4f9da4917597f7d40bdfffc476eab7f02bc3d714827d4c806f126d1eeb654fc7`；本地清单 `.pi/tmp/profiler-source.sha256`，文档和临时产物不参与。
- `cargo build --release --example perf_native --features native-profiler` 通过；构建产物 SHA-256 为 `a3be751b16c1c4c5e751f5ea07ba30005e56dedee28cfc1ab4454588820dc9cc`。只构建，没有运行原生窗口。

## 原因确认与修复边界

在改动旧断言的期望之前，通过只读 buffer 状态遍历增强诊断并定向复现同一 headless 失败，step 30 Kill 的实际残留为：

- `*scratch*`：无路径、非消息历史、可写、clean。
- `*Messages*`：无路径、真实消息历史、只读、clean。

这与 D1 的保存消息历史一致，不是文件或 view 泄漏。诊断日志 `.pi/tmp/profiler-lifecycle-identify.log` 保留；E0 的原始与基线失败日志也保留，不覆盖旧结论。

修复不是把末尾总数从 1 改成宽松上限：

- Rust workload 在每个完成步骤核对原始 scratch 身份及空 / clean 状态、fixture 的稳定 buffer 身份与路径、首次保存后的一个真实且只读 / clean 的稳定消息历史；未知 buffer、伪 `*Messages*`、缺失 / 重建历史或错误 fixture 身份均拒绝。
- 原来关闭 fixture view 后最多十个 frame-demand tick 的释放检查不变。最终完整 workload 必须没有 fixture，只剩 scratch 与真实消息历史；两步 open diagnostic 则为 scratch 与首个 fixture，不应有消息历史。
- CSV 保留总数，并新增 `scratch_buffers`、`file_buffers`、`message_buffers` 分类。Python 逐行校验、最终汇总及 fixture 同步；缺列、错误分类 / 总数或关闭 view 仍保留均 fail-closed。旧报告不因缺列而获兼容放行，不要求重跑历史批次。
- 应用层只新增 Rust 只读状态遍历、公开已有消息历史类别读取；没有新增 JS 接口，也没有为了测试去关闭或隐藏产品日志。

## 验证结果

| 检查 | 结果 |
|---|---|
| `cargo test --all-targets` | 470 项通过，0 失败 / 忽略 / 排除 |
| `cargo test --all-targets --features native-profiler` | 501 项通过，0 失败 / 忽略 / 排除；未使用 skip |
| 默认 / profiler `cargo clippy --all-targets -- -D warnings` | 均通过 |
| `cargo fmt --check` / `git diff --check` | 通过 |
| `tests/test_*.py` 离线测试 | 51 项通过 |
| manual fixture 工具离线测试 | 16 项通过 |
| `fish --no-execute scripts/acceptance.fish` | 通过 |
| release profiler example 构建 | 通过，未执行原生窗口 |

原失败项完整执行 31 步，包含三文件及三个 reopen / insert / save / kill 周期；两步 diagnostic 同样通过。定向回归共 7 项，覆盖未知普通 / 文件 buffer、只读伪日志、日志关闭后缺失与重建身份、错误 fixture 身份，以及报告末行精确分类和关闭 view 计数。Python 专项 36 项包含缺少分类、过早 / 缺失 / 多余日志、错误 scratch / 文件 / 总数的拒绝与原预算检查。

本阶段自检通过后集中 reviewer 一次，限定生命周期工具、Rust 状态读取及 Python 协议；无待修发现。审查另复跑上述 7 项 Rust / 36 项 Python 专项，无第二次审查。

原始通过日志：`.pi/tmp/profiler-default-tests.log`、`profiler-feature-tests.log`、`profiler-default-clippy.log`、`profiler-feature-clippy.log`、`profiler-python-tests.log`、`profiler-manual-tests.log`、`profiler-release-build.log`。审查生成的 Python 缓存已移至 `.pi/tmp/profiler-review-cache/`，未进入源码或替换测试结果。

## 结论与未测边界

旧 M2 生命周期自检及 Rust / Python 报告分类不一致已修复，完整 feature 测试现在实际通过；可以移除当前已知失败状态，但不改写 E0 / E1 当时的未通过记录。

本批仅验证工具逻辑、生产 shell 的 headless 功能与构建。合成 profiler 边界不提供原生时延、字体、GPU、RSS 长期泄漏或真实 IME 通过证据；没有运行 `acceptance.fish perf` 或其他真实环境矩阵，也不自动追加宿主测试。
