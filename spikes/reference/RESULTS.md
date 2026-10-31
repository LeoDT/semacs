# Emacs 31.1 对照基准（真实 command loop）

驱动：`drive.py` 在 pty 中启动 `emacs -Q -nw -l baseline.el`，按键作为终端输入注入，
`post-command-hook` 每执行一条命令后把 text/point/mark/active 追加到 jsonl。
环境：GNU Emacs 31.1、`-Q`、fundamental-mode、transient-mark-mode 1、`buffer-undo-list` 清空后开始。
（dumps 里的 index 1 是启动时 *scratch* 的一条噪声记录，忽略。）

| 用例 | 操作 | 结果 |
|---|---|---|
| undo-typing | `abc` → C-/ | 文本清空：**连续自插入合并为一个 undo 单元** |
| undo-del | `abc` → DEL×3 → C-/ | 文本恢复 `abc`：**连续 DEL 合并为一个单元**；再 C-/ → 空 |
| undo-cd | `abc`、C-a → C-d×3 → C-/ | 恢复 `abc` point=1：**连续 C-d 合并** |
| undo-move | `abc` → C-f → `def` → C-/ | 只撤销 `def`；再 C-/ → 空：移动打断合并 |
| undo-ret | `abc` → RET → `def` → C-/ | 只撤销 `def`；再 C-/ → `abc`：RET 形成边界 |
| undo-yank | `abc`、C-a → C-k → C-y×2 → C-/ | C-/ 只去掉第二次 yank（`abcabc`→`abc`）：**连续 yank 在 Emacs 里也是分开的单元** |
| undo-then-redo | `abc` → C-/ → C-M-_ | 文本恢复 `abc`，**point=1**（不是末尾） |
| del-then-undo-then-type | `abc`、C-a → C-k → C-/ → `z` → C-/ | `abc`→`zabc`→`abc`：undo 后输入是新单元，不复活旧内容 |
| undo-empty | 空历史 C-/ ×2 | 文本/point 不变，无报错状态变化（echo 未采集） |
| mark-quit | `hello world`、C-a → C-SPC → C-f×2 → C-g | 结束后 point=3、**mark=3**、active=false：**C-g 把 mark 设成 point** |
