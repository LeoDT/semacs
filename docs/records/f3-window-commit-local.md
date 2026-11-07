# F3 临时 window 与普通 commit：本地验证

本记录冻结阶段集中审查修复后的本地结论，不随以后代码更新。有效范围见 [F3 契约](../emacs-fidelity.md#阶段-f3临时-window-与普通-commit)，来源身份见 [上游参照](../work/f3-window-commit.md)，阶段收尾与宿主验证选择仍从[计划](../plan.md)进入。

## 源码身份与环境

- 基础 HEAD：`24d996e49a8bcb71ffa7c20f7907b4d3724804c8`。测试覆盖该 HEAD 上累计的未提交 F3 工作区，包括新增文件；不能把基础 HEAD 单独视为已测实现。本轮未提交或重置工作区。
- 产品源码 / 测试 / 工具清单 SHA-256：`54d2ab4856a6b8ccae11bd1a3afd4a5a318b42a78abaeeb533621f1d7fc87fed`，共 279 个文件。清单从 `git ls-files --cached --others --exclude-standard` 去重，取 `src/`、`tests/`、`scripts/`、`examples/`、`types/`、`assets/` 和根目录 `Cargo.toml`、`Cargo.lock`、`build.rs` 下现存非 Markdown 文件，按路径排序。每行为文件 SHA-256、小写十六进制、两个空格、相对路径、LF；上述摘要是整个 UTF-8 清单的 SHA-256。文档后续提炼不改变该源码身份。
- 环境：Linux `7.2.11-arch1-1`，x86_64；Git `2.56.0`；rustc `1.99.1 (58cf8f9c3 2026-10-13)`；Cargo `1.99.1 (5f94df478 2026-08-27)`。
- 功能验证使用 GPUI TestPlatform / TestTextSystem、`.pi/tmp/` 可丢弃仓库与实际编译的 semacs helper。commit 集成验证的真实 Git 子进程清空用户环境、隔离 HOME / Git 配置，只运行 fixture hooks / 签名程序，不访问日常仓库、hooks、密钥或桌面。

## 最终检查

| 检查 | 结果 |
|---|---|
| `cargo test --all-targets` | 通过，29 个测试入口共 762 项；其中 library 499 项，commit-process 集成入口 22 项 |
| `cargo test --all-targets --features native-profiler` | 通过，31 个测试入口共 835 项；其中 library 501 项，commit-process 集成入口 22 项 |
| default / native-profiler 的 `cargo clippy --all-targets … -- -D warnings` | 均通过 |
| `cargo fmt --check`、`git diff --check` | 通过 |
| `test_magit_status_manual.py` / `test_magit_fixture.py` 工具自检 | 17 / 10 项通过 |
| `python3 -B -m unittest discover -s tests -p 'test_*.py'` | 110 项通过；包含驱动替身，不代表运行了原生性能矩阵 |
| `fish --no-execute scripts/acceptance.fish` | 通过 |
| `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts` | 通过 |

计数是测试框架报告，不表示独立产品场景数量；隔离 child 的入口保护和 example 测试也包含在总数中。原始日志留在本地 `.pi/tmp/f3-closeout-final-{default,native,clippy,native-clippy,tools}.log`，不是长期规格；更早同目录失败日志不能替代本次最终检查。

## 功能证据范围

- 临时显示的复用 / 下方 split / 当前 window 回退、局部 `q`、手动布局变化、来源关闭及迟到打开隔离；原 status / diff、split、脚本及保存 / 退出回归包含在 all-targets 中。未重新测量 release diff / status 性能基线。
- 真实 Git → 私有 helper / IPC → 消息 UI → 既有异步保存 → editor 答复 → Git 终态 / status 刷新；覆盖普通 M-x 入口、默认 ask-to-stage、merge 询问、替代 index、取消与编辑前后 hook 失败、消息 ring、隐藏 / 另存 / reload 和迟到结果归属。
- 底层隔离矩阵核对 hooks / template / cleanup、unborn / detached / linked worktree、测试签名程序调用、特殊路径字节、PTY 输入输出、背压与所有权回收。16 秒编辑等待直接证明没有套用 F2 的 15 秒终止期限；不外推为任意环境的时延保证。
- 仓库进程 buffer、输出锚点 / 连续 UTF-8、两级控制、非密码提示 / 默认值 / 取消、终态刷新及 SIGHUP 退出协调。真实 Git 验证正常挂断和 helper 回收；捕获 / 忽略 SIGHUP、不自动强杀及继续编辑后再退出由隔离进程替身验证，不冒称实际 Git / 用户 hooks 会忽略 SIGHUP。

## 集中审查与修复

本阶段集中 reviewer 一次，识别四项可重现问题；主代理修复并执行上述完整检查，没有追加 reviewer，也没有将修复后自检冒充二次审查：

| 发现 | 修复及回归 |
|---|---|
| 已访问消息 buffer 丢弃新载入内容，clean 时实际提交消息与显示不一致 | 依 Emacs 默认文件复访流程询问重读，clean / dirty 不静默覆盖；保留拒绝、取消及原保存冲突处理。覆盖共享 view、后台读入、确认期间新编辑、终态解除读入和重读后 companion 焦点；真实 Git prepare hook → 重读 → finish 核对实际提交主题。上游不是无条件自动覆盖 clean buffer。 |
| 退出等待的新编辑复用旧放弃确认而丢失 | 放弃确认绑定已确认 dirty 文本快照，实际关闭前重新检查；headless 覆盖原本 clean / dirty 两种情况，忽略 / 捕获 SIGHUP 的隔离替身覆盖等待期间新增输入后重新询问。 |
| `diff.suppressBlankEmpty=true` 使 commit 预览解析失败 | 机器解析读取保留必要格式设置，不改变真实 commit 的 Git 环境；projection / commit 两种读取及带空行上下文的 M-x ask-to-stage 链路通过。 |
| 消息 buffer 从 `.git` 或自定义路径错误发现仓库，无法查看所属进程 | 优先使用 editor 请求所属会话的固定仓库；真实普通仓库、linked worktree 和 hook 自定义 editor 文件均能查看进程并局部返回消息编辑。 |

## 结论与未测边界

F3 已确认范围的实现、集中审查修复及本地检查完成，本记录不宣布宿主验收或所有性能预算通过。密码 / passphrase 读入由用户明确排除于 F3，不是未完成的阶段任务；未实现保密读入，也未以普通文本提示冒充。

没有运行真实 Wayland 窗口 / 焦点 / 平台关闭 / IME、实际 pinentry / 签名工具交互、原生 CPU 性能、GPU 或屏幕呈现测试。模拟 IME、fixture 签名调用和 headless 分屏不能替代这些证据。是否做最小定向宿主验证，或接受现有证据与明确未测边界后完成阶段归档，须由用户确认；历史 F2 的宿主授权和完成结论不自动延续。
