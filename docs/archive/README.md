# 已完成阶段归档索引

本目录保存已完成阶段及阶段间小增强的交付结论与当时未测边界，定稿后不追随后续代码更新。当前工作与跨会话待办以 [实施计划](../plan.md) 为唯一入口，有效用户行为以 [Emacs 保真度](../emacs-fidelity.md) 为准，固定版本验证与诊断证据见 [冻结记录索引](../records/README.md)。

## 已完成阶段与增强归档

| 归档 | 涵盖范围 |
|---|---|
| [R0–F1 归档](completed-through-f1.md) | R0–M3、阶段 A–E3、日常编辑增强（双向 word kill / 段落移动）、F1 mode 与只读 Magit diff |
| [F2 归档](f2-status.md) | F2 status 与单文件 index 操作 |
| [F3 归档](f3-window-commit.md) | F3 临时 window 与普通 commit |
| [F4 归档](f4-status-hunks.md) | F4 status 内嵌 diff 与单 hunk index 操作 |
| [F5 归档](f5-history-commit.md) | F5 本地提交历史与提交整理 |
| [F5 后插入项归档](post-f5-insertions.md) | Magit 样式 / M-x 历史 / scratch 退出、局部强调重排、鼠标基础选区与路径预填、正文外鼠标命中 |
| [阶段间小增强归档](enhancements-through-f11.md) | F6 前小增强（recenter / 滚轮横滚 / find-file 候选 / `C-x C-k` / `S-TAB`）；F11 后增强（Magit other-window / buffer 轮换、系统剪贴板） |
| [F6 归档](f6-tree-viewer.md) | F6 文件夹 tree viewer |
| [F7 归档](f7-project.md) | F7 基础 project |
| [F8 归档](f8-search-rg.md) | F8 ripgrep 搜索（项目与文件夹） |
| [F9 归档](f9-isearch.md) | F9 当前 buffer / diff 增量搜索（`isearch`） |
| [F10 归档](f10-multiple-cursors.md) | F10 多选区同步编辑（`multiple-cursors`） |
| [F11 归档](f11-diff-range.md) | F11 diff range / revision |

## 已归档上游参照

以下材料仅作已完成阶段的静态源码 / 外部对照核对入口，不是 semacs 行为契约或验收通过记录：

| 参照 | 对应范围 |
|---|---|
| [Magit diff 行为参考](references/f-magit-reference.md) | F1–F5 Magit section / diff / 折叠与导航参照 |
| [F3 上游参照](references/f3-window-commit.md) | F3 window 与 Git editor 生命周期参照 |
| [F5 上游参照](references/f5-history-commit.md) | F5 提交历史与提交整理参照 |
| [Treemacs 参照](references/treemacs-reference.md) | F6 文本树与文件浏览参照 |
