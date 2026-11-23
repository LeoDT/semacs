# 已完成阶段归档：F10 多选区同步编辑（`multiple-cursors`）

本页保存 F10 收尾时的交付结论和未测边界，不追随后续代码更新。有效行为以 [F10 契约](../emacs-fidelity.md#阶段-f10多选区同步编辑multiple-cursors) 为准；当前工作和重要未完成事项以[计划](../plan.md)为准。

## 交付结论

参照 Emacs `multiple-cursors.el` 的交互与约定，在普通可编辑正文 buffer 上交付了多选区同步编辑与批量原子撤销：

- **管理命令与绑定**：`mc/mark-next-like-this`（`C->`）、`mc/mark-previous-like-this`（`C-<`）、`mc/mark-all-like-this`（`C-c C-<`）、`mc/edit-lines`（`C-S-c C-S-c`）、`mc/skip-to-next-like-this` / `mc/skip-to-previous-like-this`、`mc/unmark-next-like-this` / `mc/unmark-previous-like-this` 及 `C-S-<mouse-1>` 鼠标增删光标（不改内置 `C-d`）。
- **同步编辑与批量原子历史**：支持全光标同步移动、设 mark、Emacs 风格删除（不启用 `delete-selection-mode`，`Backspace` / `Delete` 删除全部活动选区）、`C-j` 换行、`TAB` 缩进、每光标独立局部 `kill-ring` 与 `C-y` 粘贴、以及 IME 最终提交同步插入全部光标；扩展历史系统支持多处非重叠编辑作为单一 undo 组原子回退/重做并合并连续同类输入/删除，正确维护 `SavePoint`、`modified` 与分屏 marker 位移。
- **生命周期与视觉反馈**：保持上游默认 `RET` 退出多光标、`C-g` 两段式取消与 `ESC ESC ESC` 直接退出；重合光标自动去重合并；次光标复用 `cursor` 样式（含同行多光标字符反色），活动次选区复用 `region` 样式，mode line 显示 `mc:<数量>`；多光标归属当前 view，只读 / minibuffer / section buffer 明确拒绝，进入 `isearch` 或普通左键点击自动退出。

## 验证依据与边界

本地完成了 `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、新增批量历史单测 / 多光标核心单测 / GPUI headless 视图与 `AppShell` 集成测试、全量库单测及核心集成测试（含 `tests/keymap.rs` 绑定校验），并完成一次集中代码审查及全部 9 项审查问题修复与回归测试（涵盖同行多光标 `C-k` 合并崩溃防护、紧邻批量编辑单次 D0→D1 marker/mark 映射、非最小偏移主光标 redo 定位、显式 range 与前驱光标下的 IME `DisplayChange` 增量重排不变量、`skip` 无更多匹配保留原光标、`mark-all` 重叠推进、分屏只读同步退出、全路径光标去重与多行移动原子性）；用户随后在本地简单试用确认无异常。

本阶段未进行正式宿主 GUI 验收记录、性能基准测试、真实桌面 IME 或跨平台测试；在无特权沙箱内 `commit_process` 的 2 项 `SIGHUP` 信号退出测试受沙箱环境影响未在本次复核。首版不提供的上游能力以 [F10 契约](../emacs-fidelity.md#首版不提供的上游能力-4) 为准。
