# E2 定向宿主验证工具：本地准备

## 身份与范围

- 基线 `8e680a0d78b35634ad278d6ff27664dd2b054371` 加 E2 产品实现及本轮工具未提交差异；产品实现的先前证据见 [E2 本地验证](e2-languages-local.md)，不改写其源码身份、测试数量或未测边界。
- 环境：Linux x86_64，kernel `7.2.11-arch1-1`，Rust `1.99.1`。本轮只执行离线检查、模拟 frame 与 GPUI TestPlatform 测试，没有运行原生应用、访问桌面、截屏或切换输入法。
- `Cargo.toml`、`Cargo.lock`、`THIRD_PARTY.md` 及 `src/`、`tests/`、`examples/`、`scripts/`、`types/` 下 Git 跟踪 / 非忽略新增文件共 172 项，排序 SHA-256 清单摘要 `0129ace7b813997ea540017f6325a91918071a94a9b12299530703bed6ee0163`，清单 `.pi/tmp/e2-host-tools-source.sha256`；文档 / 临时产物不参与源码清单。
- release `native-profiler` 的 `examples/perf_native` SHA-256：`9b4541e3907fcf905615c60a82cccc23a6b0c645b2c59c6e5d8ef406fd0f59ec`。无 `RUSTFLAGS`、`CARGO_ENCODED_RUSTFLAGS` 或 `CARGO_BUILD_TARGET` 覆盖。宿主入口仍会重建并保存当批身份，不能直接借用本条构建身份证明宿主结果。

用户批准的是工具准备：一个人工窗口及 Markdown / HTML 各约 256 KiB、各三轮原生 CPU 批次。具体入口与行为边界见 [验收流程](../acceptance.md#e2-定向原生-cpu-批次)；尚未授权或运行宿主批次，不将批准准备等同真实环境运行同意。

## 已核定的覆盖缺口

旧 `syntax` 人工模式仅覆盖 Rust；原生性能主路径创建无路径 buffer，且只观察布局 ready；旧 `split-ime` 的 fixture 也为普通文本。因此没有直接运行旧入口并将结果记作 E2 覆盖。

新入口：

- `languages`：一个隔离配置窗口，由用户完成两处嵌入 script 注释的中文预编辑、取消与提交，以及颜色 / 分屏 / 主题 / 历史 / 保存观察；独立核对两个文件各新增一个“你”及配置字节。不自动注键、不切换输入法、不截图；IME 结论是用户观察，不是自动 IME 状态或时延断言。
- `syntax-perf`：六个窗口依次运行，使用带语言路径的确定性 Markdown / HTML fixture；在嵌入 JavaScript 中输入，分别记录布局和当前快照语法就绪，保留中间版本语法未观察状态。最后版本着色完成后才测稳定 draw / idle，未观察到旧语法 pending 时继续输入则报覆盖缺失，不自动改样本或重试。

这两个入口不改变产品行为或性能预算，也不替代大文件全矩阵、持续输入、真实 IME 时延、GPU 执行或实际呈现验证。

## 离线检查

- `cargo test --all-targets`：488 项通过；`--features native-profiler`：526 项通过，均无失败 / 忽略。
- 默认 / profiler 两套 Clippy（`--all-targets -- -D warnings`）、`cargo fmt --check`、`git diff --check`、fish 语法检查通过。
- `python3 -B -m unittest discover -s tests -p 'test_*.py'`：67 项通过；人工 fixture 工具测试：23 项通过。
- release profiler 应用 / `perf_native` 构建通过。没有运行原生 example 或软件渲染诊断。

新增 Rust 检查覆盖：布局与语法时限分离、精确同帧目标、丢失 / 空白帧、deadline fail-fast、idle、输入 marker，以及实际 TestPlatform fixture 启用后台语法并跟随当前快照。Python 检查覆盖报告列 / 阶段 / 字节 / point / 版本 / 时序 / 预算、缺失 pending 覆盖、配置隔离、失败停止 / 恢复、整批 fixture 身份和原有模式回归。假子进程只验证启动参数、隔离环境及报告协议，不冒充真实 renderer 或原生计时。

最终日志：`.pi/tmp/e2-host-final-default-tests.log`、`e2-host-final-feature-tests.log`、`e2-host-final-python-tests.log`、`e2-host-final-manual-tests.log`、`e2-host-final-default-clippy.log`、`e2-host-final-feature-clippy.log`、`e2-host-final-release.log`。

## 工具阶段集中审查

本阶段自检后集中 reviewer 一次，发现三项 P2，主代理已修复并补离线回归：

1. pending 输入覆盖与前一阶段 ready、单帧 pending / ready 可能矛盾：validator 现核对跨阶段状态，ready 行必须确有非 pending 帧，不靠孤立布尔值生成覆盖结论。
2. 全文首个同名 `call(42)` 可能不在嵌入代码内：Rust / Python 都改为完整 script marker 定位，Markdown 额外要求围栏上下文；前方另有同名普通文本也不能改变输入目标。
3. 后一种样本可能在前一种运行期间变化并重新建立基线：生成时冻结两份文件的统一 manifest，fish 向两个进程传递同一个摘要，每轮前后验证整个批次；后续进程不能接受改变的文件或被一起更新的 manifest。

修复后完整自检通过，没有追加 reviewer，没有遗留已知工具必修项。最终结果仍是“入口已准备”，不是宿主功能 / 性能通过。

## 下一步

取得对两个具体批次的宿主运行授权后，由用户在正常 fish / Wayland 终端运行新目录入口；任一失败 / 未观察 / 覆盖缺失停止后续测试，保留全部目录回传，再按当批身份和证据判定。尚未进入 F。
