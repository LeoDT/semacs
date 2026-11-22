# 已完成阶段归档：F7 基础 project

本页保存 F7 收尾时的交付结论和未测边界，不追随后续代码更新。有效行为以 [F7 契约](../emacs-fidelity.md#阶段-f7基础-project) 为准；当前工作和重要未完成事项以[计划](../plan.md)为准。

## 交付结论

按 GNU Emacs 31.1 内置 `project.el` 语义完成了项目识别、已知项目列表持久化（`$XDG_STATE_HOME/semacs/projects` 或 `$HOME/.local/state/semacs/projects`）、`C-x p` 前缀与核心命令（`project-switch-project`、`project-find-file`、`project-switch-to-buffer`、`project-kill-buffers`）及已知列表管理命令（`project-remember-project`、`project-forget-project`、`project-forget-zombie-projects`、`project-remember-projects-under`）。

- `project-switch-project`（`C-x p p`）采用已批准的单键分派菜单：`[f] Find file`、`[b] Switch buffer`、`[v] Magit status`、`[t] Treemacs`。
- `C-x p p` 选 `[t]` 时把左侧 treemacs 树切换到目标项目根；选 `[f]` 打开另一项目文件时保持已打开树根不变；从 treemacs 侧窗触发打开/切换 buffer 时自动切到普通 pane。
- 上游默认键位参照已导出至 [f7-bindings.tsv](../../tests/emacs/f7-bindings.tsv) 并纳入 `tests/keymap.rs` 核对。

## 验证依据与边界

本地完成了 `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、744 项 `lib` 测试（含 `project` 单元测试与 GPUI headless 集成测试）、受影响的集成测试以及一次集中代码审查与审查修复；用户随后在本地简单试用确认无异常。

本阶段未进行正式宿主验收记录、性能基准测试、真实 IME 或跨平台测试；在忽略 `SIGHUP`（`SigIgn=1`）的沙箱中，依赖 `SIGHUP` 终止子进程的 2 项历史 `tests/commit_process.rs` 用例未测（本阶段未改动 `commit_exit` 逻辑）。首版排除的上游 `project.el` 能力以[F7 契约](../emacs-fidelity.md#首版不提供的上游能力-1)为准，后续搜索 / diff range 等方向仍需另行确认。
