# Magit 全局 section cycle 行为观察

固定环境下的上游行为证据，不是 semacs 通过记录，也不表示其他 mode 已实测。

## 环境

- Emacs 31.1（`/usr/share/emacs/31.1`）。
- magit-20260822.1158（commit `137f137dccb37097ae9caa31018cafa071ff4ec1`）、magit-section-20260731.2248（commit `67f203853e74e926e2c99f60ed508840714f7ced`）。
- 临时仓库：`a.txt` 已修改、`b.txt` 已 staged、`c.txt` untracked；`emacs -Q --batch` 载入上述包后 `magit-status`，直接调用 `magit-section-cycle-global`，用 section 的 `hidden` 槽、invisible overlay 与 `magit-section-hidden-body` 判定可见状态。

## 结果

- 部分折叠时第一次 `S-TAB`：所有 section `hidden` 变 nil、invisible overlay 清零，完全展开。
- 第二次 `S-TAB`：只有 root 的直接子 section `hidden`=t，其子孙仍为 nil，正文不可见。
- 第三次 `S-TAB`：回到完全展开。
- 全部展开后单独 `TAB` 折叠一个 hunk，再 `S-TAB`：全部展开，不折叠顶级；对应 `magit-section-cycle-global` 的第二分支。
- 折叠时 point 位于被隐藏的正文内：`magit-section-hide` 把 point 移到该 section heading（probe 中 114 → 89）。

## 结论

可见状态是两态循环：仍有正文隐藏时全部展开；全部正文可见时只折叠顶级子 section。命令的三个 cond 分支中前两支都导致完全展开，因此此前记录的“三态 cycle”不成立。

## 未测边界

- 只在 batch 下观察 `magit-status`；未重复 diff / process / log / log-select / revision，这些 mode 共用同一 `magit-section-mode` 命令与 keymap。
- 未观察真实 GUI 下的重绘时序、point 与滚动锚定的交互。
