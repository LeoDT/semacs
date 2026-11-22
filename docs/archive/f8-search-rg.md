# 已完成阶段归档：F8 ripgrep 搜索（项目与文件夹）

本页保存 F8 收尾时的交付结论和未测边界，不追随后续代码更新。有效行为以 [F8 契约](../emacs-fidelity.md#阶段-f8ripgrep-搜索项目与文件夹) 为准；当前工作和重要未完成事项以[计划](../plan.md)为准。

## 交付结论

参照 GNU Emacs 31.1 内置 `grep-mode` / `compile.el` / `project.el` 与 `rg.el`，交付了基于 `rg --json` 的项目内搜索与任意文件夹搜索：

- **入口与输入**：`C-x p g`（`project-find-regexp`）、`M-x rg-project`、`C-x p p` 菜单 `[g] Find regexp` 与任意目录搜索 `M-x rg`；支持光标处默认标识符、正则 + `smart-case` 默认语义与目录候选补全。
- **流式结果 buffer（`*rg*` / `rg` mode）**：后台异步流式运行 `rg --json`，按文件分组生成可折叠 section 树与命名样式高亮（`search.file`、`search.line-number`、`search.match`），受有界结果与 stderr 排空保护。
- **导航与重搜**：`rg` mode 支持 `RET`、`C-o`、`n` / `p`、`M-n` / `M-p`、`{` / `}`、`TAB` / `S-TAB`、`g`、`r` / `t` / `d` / `f` / `c` / `i`、`C-c C-k`、`q`、翻页与水平滚动；全局支持 `M-g n` / `M-g M-n` / `C-x \``（`next-error`）与 `M-g p` / `M-g M-p`（`previous-error`）。

## 验证依据与边界

本地完成了 `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、完整单元测试与 GPUI headless 集成测试（涵盖 `rg` 实时执行、JSON 解析、glob 花括号、`C-x p g` / `C-x p p` `[g]`、`M-x rg`、全部重搜与导航键、另存脱离及 kill 清理），以及一次集中代码审查与审查修复（取消与 PID 复用锁保护、stderr 排空、`*.{a,b}` glob、子目录 `default-directory`、Treemacs 侧窗跳转、多视图点位同步与退出取消保护）；用户随后在本地简单试用确认无异常。

本阶段未进行正式宿主 GUI 验收记录、性能基准测试、真实 IME 或跨平台测试；非 Unix 平台未实现活动扫描进程组的即时外部强杀。首版未包含的上游 `grep-mode` / `rg.el` 能力以 [F8 契约](../emacs-fidelity.md#首版不提供的上游能力-2) 为准。
