# F5：提交历史与提交整理本地验证

## 固定身份与范围

- 源码基点：`f6af5e38f115080b546df561351b817de3764918` 加本轮工作区修改；不是已提交的新版本。
- 纳入新文件的源码清单 SHA-256：`f985ec90d826720982ed7bb7882a0375ee8a09e005dc05e7aa649001a986ce6e`。清单生成入口：`git ls-files -co --exclude-standard -z -- Cargo.toml Cargo.lock src types tests examples scripts | LC_ALL=C sort -zu | xargs -0 sha256sum`；本地清单保留于 `.pi/tmp/f5-source-manifest.sha256`。该清单包含 untracked 新源码；性能驱动的普通 `source.patch` 单独不能替代它。
- release 桌面二进制 `target/release/semacs` SHA-256：`3bcf01f561fa8aff1504f963fd5a736a0eabcbdfa0ba2a6cab6ac81ed887080c`；仅构建，未启动。
- 本机 Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Git `2.56.0`；GPUI TestPlatform / TestTextSystem，Git / helper 用 `.pi/tmp/` 下可丢弃仓库及隔离子进程，不启动桌面、不读写日常 Git 配置 / hooks / 工作文件。
- 批准范围及有效行为见 [F5 契约](../emacs-fidelity.md#阶段-f5本地提交历史与提交整理)，不在此另维护功能规格。Magit 固定参照及 13 项外部 batch 对照见 [上游入口](../work/f5-history-commit.md) / [外部结果](../../tests/emacs/RESULTS.md#f5magit-提交与历史参照)，不是 semacs 通过记录。

## 自动检查

| 入口 | 本轮结果 |
|---|---|
| `cargo test --all-targets` | 全部通过；其中 lib 546 项，commit_process 26 项（含隔离 child 入口）。 |
| `cargo test --all-targets --features native-profiler` | 全部通过；其中 lib 548 项，commit_process 26 项，并覆盖 feature 专有测试。 |
| 默认及 native-profiler 的 `cargo clippy --all-targets … -- -D warnings` | 均通过，无抑制新增警告。 |
| `cargo fmt --check` / `git diff --check` | 通过。 |
| `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts` | 通过。 |
| `cargo build --release --bin semacs` | 构建完成；未运行该桌面二进制。 |

覆盖包括：

- 真实 Git 的当前分支 / detached 日志、graph / 元数据 / 数量限制，刷新及共享 buffer 的独立 point、失败保留原投影身份，root / 普通 / 两及三 parent combined 的固定 blob 访问。
- TestPlatform 正式命令路由：日志 RET / q、revision 文件访问、parent 候选读入、commit 菜单及发布确认取消、fixup 选择导航 / 取消、merge-range 中止、kill / 导出 / 新读入后的迟到结果、终态与旧日志读取隔离。
- 隔离真实 Git / semacs editor helper 的 9 类 F5 场景：amend / reword 含 staged 和空 staged、普通 fixup / squash、无 staged 的 fixup `--all`、fixup hook、squash hook 失败。独立核对 HEAD tree、完整 index tree、worktree、untracked、提交数 / 消息及 helper 回收；编辑消息期间重新调用 commit 菜单仍复用消息 buffer。
- 另有 fixup / squash 的替代对象库场景：对象只存在于 `GIT_OBJECT_DIRECTORY`，覆盖从普通文件发现仓库、选择日志、revision RET、固定 blob 访问及真实提交。没有通过关闭对象库隔离或回落到父仓库来使测试通过。
- 继承的 F3 进程 / editor / 提示 / 退出、F2/F4 index 写入和脚本 / 键位回归亦随全目标执行；不扩展成所有外部 hooks / 签名工具已覆盖。

默认 verbose 参数暴露旧信号替身仅以“最后一个参数是否 commit”识别启动的缺陷；替身已改为识别 commit 命令并允许后续参数。SIGHUP、不自动升级强杀、退出等待等产品语义未改，原有信号回归通过。

## 一次集中审查及主代理修复

阶段主体和自检完成后调用一次 reviewer；没有提前或追加审查。报告三项，均由主代理修复并补测试：

1. **dense-combined 合法省略 patch**：raw 元数据与 patch 按解码后的 Git 路径关联，区分 hunks / 省略 / 非文本摘要；全部或部分 patch 被省略均保留 parent / result 版本。真实 fixture 同时覆盖“只省略”和“前一个文件省略、后一个文件有 hunk”，后者含 Tab / 引号 / 非 ASCII 路径，防止取消数量检查后错位 zip。
2. **目标选择丢失 commit 对象库环境**：日志 query 显式携带环境，贯穿分支发现、读取、revision / blob 和复用身份；提交入口 / 进程查看的仓库发现也沿用 commit 环境，避免仅在外部对象目录下成立的仓库被误判后回落父目录。普通投影仍隔离实际 commit 环境。
3. **日志输出编码**：固定 `--encoding=UTF-8`；非 ASCII 作者 / 摘要及 `i18n.logOutputEncoding=ISO-8859-1` 下，普通日志和选择日志均通过。

另对可见 / 待完成 revision 身份隔离及读取环境不可跨 buffer 复用补充回归。审查后的修复未再次调用 reviewer，最终默认 / feature 全量检查由主代理执行。

## Headless 独立 diff 回归

最终入口：`python3 -B scripts/perf_diff.py .pi/tmp/f5-diff-regression-02`，release + native-profiler；约 256 KiB / 2 MiB patch 各三轮，包含 staged / unstaged、折叠导航与双 view。

144 行观测均为 ready，正文及可见投影 oracle 通过，无缺失观测或 blank frame。三轮 open 的 all-ready 范围与同类动作最大 draw CPU：

| 样本 / 比较 | open all-ready（ms） | 最大 draw CPU（ms） |
|---|---:|---:|
| small staged | 32.168–32.255 | 0.215 |
| small unstaged | 11.535–11.635 | 0.235 |
| large staged | 47.120–47.414 | 0.219 |
| large unstaged | 45.468–45.739 | 0.215 |

这是继承的独立 diff 路径回归及 TestPlatform CPU 基线，不是新日志 / revision 的原生性能测量。驱动明确 `budget_passed=None`，不将完成采集或以上数字解释为总体性能预算通过；不测原生 idle、字体冷路径、GPU 或屏幕呈现。审查前另一次 `.pi/tmp/f5-diff-regression-01` 仅对应当时源码，不用它替代最终身份。

## 结论与未测边界

本轮实现、一次集中审查修复和本地回归完成，无已知阻塞性遗留；没有自动提交或启动真实桌面。日志 / combined 新界面的真实字体 / 可读性、原生键盘 / 窗口焦点和新流程的宿主操作尚未观察；没有执行日常签名工具 / 密码交互或原生性能矩阵，不据 headless 测试外推。用户试用 / 宿主验证与提交收尾仍从[计划](../plan.md)进入，真实环境验证须另行取得具体授权；历史已接受的观察及原预算不变。
