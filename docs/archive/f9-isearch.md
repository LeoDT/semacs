# 已完成阶段归档：F9 当前 buffer / diff 增量搜索（`isearch`）

本页保存 F9 收尾时的交付结论和未测边界，不追随后续代码更新。有效行为以 [F9 契约](../emacs-fidelity.md#阶段-f9当前-buffer--diff-增量搜索isearch) 为准；当前工作和重要未完成事项以[计划](../plan.md)为准。

## 交付结论

参照 GNU Emacs 31.1 内置 `isearch.el`，交付了覆盖普通文本与结构化 section buffer（diff / status / log / revision / tree / `*rg*`）的增量搜索与正则搜索：

- **全局入口与模式**：`C-s` / `C-r`（字面量增量搜索）、`C-M-s` / `C-M-r`（Rust `regex` 语法多行增量正则搜索）、`M-s w`（词搜索）、`M-s _`（符号搜索）、`M-s .`（光标处严格符号搜索）；默认对齐 `case-fold-search`、`search-upper-case = not-yanks`、`isearch-lax-whitespace`（空格与 Tab）、`isearch-wrap-pause`、`isearch-repeat-on-direction-change = nil`。
- **完整会话交互**：支持状态栈撤销（`DEL`）、末字符删除（`C-M-d`）、两段式 `C-g` 与 `ESC ESC ESC`、抓取词/符号/行/kill-ring（`C-w` / `C-M-w` / `M-s C-e` / `C-y`）、`C-q` 字面引用、搜索历史环（`M-p` / `M-n` 与空串重复召回）、`M-e` minibuffer 编辑与空 `RET` 非增量搜索、各类实时开关（`M-c` / `M-r` / `M-s w/_/SPC/i`）、首末跳转（`M-s M-<` / `M-s M->`）及未绑定键退出后自动重放。
- **隐藏 section、高亮与 IME**：实现 `search-invisible = open` + `isearch-hide-immediately` 临时展开/立即复折/停留保留与 `M-s i` 跳过隐藏；新增颜色命名样式 `isearch` / `isearch.lazy`（视口滚动与跳屏自动刷新、不失效布局）；支持只读与可编辑 buffer 在 `isearch` 期间直接进行平台中文 IME 输入并遵守 D17 组合屏障。

## 验证依据与边界

本地完成了 `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、新增 `isearch` 单元测试与 GPUI headless 集成测试、全量库单测及 `tests/` 集成测试（含 `tests/emacs/f9-bindings.tsv` 对照校验），完成一次集中代码审查与全部 12 项审查问题的修复及回归验证（涵盖滚动/跳屏懒高亮刷新、`M-s i` 即时复折、`C-M-w` 键位、`M-s .` 空白恢复、反向正则中行锚点校验、`C-q` 与前缀键的 `prefer_character_input` 兼容、IME 注册与 D17 屏障、切 buffer / 重载 / 点击收尾、越界快照夹紧与有界懒高亮扫描）；用户随后在本地简单试用确认无异常。

本阶段未进行正式宿主 GUI 验收记录、性能基准测试、真实桌面 IME 或跨平台测试；在无特权沙箱内 `commit_process` 的 2 项 `SIGHUP` 信号退出测试受沙箱环境影响未在本次复核。首版不提供的上游能力以 [F9 契约](../emacs-fidelity.md#首版不提供的上游能力-3) 为准。
