# 已完成阶段归档：F11 diff range / revision

本页保存 F11 收尾时的交付结论和未测边界，不追随后续代码更新。有效行为以 [F11 契约](../emacs-fidelity.md#阶段-f11diff-range--revision) 为准；当前工作和重要未完成事项以[计划](../plan.md)为准。

## 交付结论

参照上游 Magit（`942d61d5`）的 `magit-diff.el`、`magit-git.el`、`magit-section.el` 与 `magit-mode.el`，在既有只读 diff、status、log 与 revision 查看器之上补齐了任意 revision 与 range 的查看能力，不扩展 Git 写工作流：

- **菜单与命令**：在 Magit 各 mode 绑定 `d`（`magit-diff` 单键菜单：`d` dwim、`r` range、`u` unstaged、`s` staged、`w` worktree、`c` show commit），在 `magit-diff` mode 绑定 `D`（`magit-diff-refresh` 单键菜单：`r` `magit-diff-switch-range-type`、`f` `magit-diff-flip-revs`、`g` 刷新）；将 `magit-show-commit` 放开到全局 `M-x`。
- **Range / revision 解析与补全**：支持 `A..B`、`A...B`（含省略单侧默认 `HEAD`）与单 rev（固定 OID → working tree）对比；所有 revision 通过 `--end-of-options <rev>^{commit}` 解引用为完整 OID 并固定比较身份；按 `refs/heads` → `refs/remotes` → `refs/tags`（排除 symref）+ 特殊 ref 提供候选补全，支持 `..` / `...` 右侧补全与手输原始表达式。
- **Log 选区与 DWIM**：在 `magit-log` / `magit-log-select` 中选中多条 commit 兄弟行后，`d d` 与 `d r` 直接打开 `<底部较旧>..<顶部较新>` 并停用 mark；支持 status 分组、log 单提交、revision 与 diff 的 DWIM 行为；不自动保存已修改编辑 buffer。
- **投影与生命周期隔离**：range / 单 rev diff 每仓库复用 `*magit-diff range: <root>*`，同比较 `g` 刷新保留折叠与滚动，切换比较或翻转时重置默认折叠与位置；读取失败保留旧投影与旧比较身份；不可变 `Range` 不被 Git 写入标为 stale 或被提交终态队列误刷。

## 验证依据与边界

本地完成了 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`git diff --check`、新增 range 解析 / OID 固定 / ref 枚举单测与 GPUI headless 集成测试（`src/app/diff_range_tests.rs`）、全量库单测（791 项）及核心集成测试（含 `tests/keymap.rs`、`tests/mx.rs`、`tests/candidates.rs`、`tests/minibuffer.rs`、`tests/m2_flows.rs`、`tests/buffers.rs`、`tests/split.rs`、`tests/commit_process.rs`），并完成一次集中代码审查及全部 6 项审查问题修复与回归测试（含翻转删除文件重置折叠/位置、`queue_commit_refresh` 跳过不可变 `Range`、`shell.exiting()` 回调防护、失败前不提前清除 log 选区、过期刷新错误抑制、非 UTF-8 当前分支降级与 `HashSet` 去重）；用户随后在本地试用确认可用。

本阶段未进行真实宿主 GUI 验收、性能基准或跨平台测试；无特权沙箱内 `commit_process` 的 2 项 `SIGHUP` 信号退出测试受沙箱环境影响未在本次复核。首版裁剪与已批准差异以 [F11 契约](../emacs-fidelity.md#已批准差异与首版裁剪) 为准。
