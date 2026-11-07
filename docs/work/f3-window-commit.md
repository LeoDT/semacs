# F3 上游参照：window 与 Git editor 生命周期

本页只保留静态核对入口与来源身份，不维护实现进度、测试矩阵或另一份行为规格。有效行为见 [F3 契约](../emacs-fidelity.md#阶段-f3临时-window-与普通-commit)，当前工作见[计划](../plan.md)，本地验证见[冻结记录](../records/f3-window-commit-local.md)。密码 / passphrase 读入已由用户排除于 F3，不是阶段收尾前置条件。

## 来源身份与参照边界

- Magit 固定参照：`942d61d5de37c89a3a7c6cf693696b900b1d97b0`（`v4.7.1-74-g942d61d5`），本地 `/home/leodt/ghq/github.com/magit/magit`。
- 补充 with-editor：本机 `with-editor-20260731.2234/with-editor.el`，包头 revision `a1f92a26e530`，SHA-256 `6ea16f15b201ab1c26b04bd4df514f915f58787786f2d58d802de5ae26dfc106`。它不是该 Magit checkout 锁定的依赖，不能混称为同一固定快照。
- 补充 Emacs：本机 31.1 的 `lisp/window.el`、`files.el`、`server.el` 和 `vc/log-edit.el`。
- 以上仅静态阅读，不加载日常配置、不运行 Emacs / 桌面 / 用户 hooks，不代表 command loop 或宿主观察；未复制或翻译上游实现，不引入这些包为运行依赖。来源义务见 [THIRD_PARTY](../../THIRD_PARTY.md)。

## 核对入口

| 主题 | 上游入口及核对重点 |
|---|---|
| 临时 window | Magit `magit-mode.el`：`magit-display-buffer`、`magit-mode-bury-buffer`、`magit-mode-quit-window`；Emacs `window.el`：`quit-restore-window`。区分已显示复用、新建 window 与替换 window 的局部历史，不恢复整棵旧布局。 |
| 普通 commit 启动 | Magit `magit-commit.el:208–219,450–496`：`magit-commit-create`、`magit-commit-assert`；`magit-git.el:1470–1495`：staged / unstaged 谓词。默认 `magit-commit-ask-to-stage=verbose` 及 merge 分支不是 staged-only 限制。 |
| Git editor 协议 | Magit `docs/magit.org` 的 Editing Commit Messages（5190 行起）、`magit-run-git-with-editor`；with-editor 的 filter / server 路径。Git 先启动、传入真实消息文件并等待 editor 答复，不是先编辑再 `commit -F`。 |
| 消息 companion | `magit-commit-diff`、`magit-display-buffer-noselect`；普通 staged 为 `--cached`，不选择 diff window。`git-commit-setup-font-lock` 从 `core.commentchar` 取得注释前缀，缺省 `#`。 |
| finish / cancel / ring | `git-commit.el`：`git-commit-setup`、`git-commit-save-message`、prev / next-message；with-editor：`with-editor-return`、`with-editor-mode`；Emacs `log-edit.el`。核对结束前保存、第二行询问、32 条共享 ring 与逐 buffer 浏览位置，以及 ring 的注释 / scissors 过滤不等于改写 Git cleanup。 |
| 复访与文件保存 | with-editor 调用 `find-file-noselect`；Emacs `files.el` 默认 `query-about-changed-file=t`、`revert-without-query=nil`。已有文件在磁盘变化时，clean / dirty 均询问是否重读；dirty 文案明确放弃编辑，拒绝不覆盖草稿。不能把 clean 自动覆盖当默认上游行为。 |
| 隐藏 / kill / 另存 | with-editor 将 `kill-buffer` 命令重映射到 cancel，其他直接 kill 路径受 query hook 拦截；会话状态为 permanent-local。`write-file` 改变访问路径，不改变 Git 已传入并等待的原路径。 |
| 输出 / 提示 / 信号 | `magit-process-buffer`、`magit-process-kill`、`magit-start-process`、`magit-process-filter`、`magit-process-make-keymap`。PTY、提示转发、读入取消杀所属进程及两级控制不是任意 TUI 宿主。密码提示是上游行为，但已明确排除于本阶段。 |
| 历史与终态刷新 | `magit-process-log-max` 默认 32 个 section，超限移除较旧的一半、保留活动进程；`magit-process-sentinel` / `magit-refresh` 刷新发起 Magit buffer 及 status，不是全部 diff。 |
| 正常退出 | Emacs `save-buffers-kill-emacs`、`server-kill-emacs-query-function`：先处理修改文件，再询问活动进程 / 等待 client。信号与 semacs 已批准的异步等待边界见契约，不把内部失去所有权回收当正常退出。 |

## 正常退出的补充在线来源

[Sending Signals to Processes](https://www.gnu.org/software/emacs/manual/html_node/elisp/Signals-to-Processes.html) 说明 Emacs 正常退出发送 SIGHUP，显式 `kill-process` 才发送 SIGKILL；[bug#57199 讨论](https://lists.gnu.org/r/bug-gnu-emacs/2022-08/msg01380.html) 说明这是为允许子进程自行收尾，进程也可能忽略 SIGHUP。

在线手册和讨论仅为补充来源，不冒称本机 Emacs 31.1 固定源码的执行证据。semacs 的 SIGHUP 及不自动升级强杀已经用户确认；真实 Git 与信号替身的验证边界分别见本地冻结记录。
