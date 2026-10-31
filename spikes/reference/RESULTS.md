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
| mark-quit | `hello world`、C-a → C-SPC → C-f×2 → C-g | 结束后 point=3、**mark=1**、active=false：C-g 只停用 region，mark 位置保留 |
| quit-no-mark | 空 buffer → C-g ×2 | 两次都回显 `Quit`；point/mark 不变 |
| minibuffer-quit | `C-x C-f` → 输入 → C-g | 取消 minibuffer，回显 `Quit`，buffer 不变 |
| buffers | A 设 mark → `C-x b foo RET` → 编辑并设 mark → `C-x b *spike* RET` | 切回 A 后 point / mark / active（1、3、true）保留；B 的状态独立；undo 只影响当前 buffer |
| undo-then-input | 见 `del-then-undo-then-type` | undo 后输入开新组，不复活旧内容 |

### C-g 与前缀（重要修正）

`C-g` 单独按始终 `Quit`。`C-x C-g` 的结果取决于 pty 是否把两个字节合并进同一次 read：

- 分开到达（真实按键间隔，GUI 也是如此）→ 回显 `C-x C-g is undefined`，**mark 位置与 active 都保留**；
- 同一次 read 到达（`C-x C-g` 连发）→ `Quit` 且 deactivate region。

因此旧记录“按 region 是否有活动区分 Quit / undefined”是采集假象，不是 Emacs 语义。
真实 PGTK GUI（Hyprland + 真实按键）复核为 `C-x C-g is undefined`。
