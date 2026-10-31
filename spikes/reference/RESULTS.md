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
| redo-point | `abc`、输入 `zz`（point=6）→ C-/ → C-M-_ | 文本 `abczz`，**point=4**（1-based）：redo 回到组内最早变更的起点，不是组末 |
| del-then-undo-then-type | `abc`、C-a → C-k → C-/ → `z` → C-/ | `abc`→`zabc`→`abc`：undo 后输入是新单元，不复活旧内容 |
| undo-empty | 空历史 C-/ ×2 | 文本/point 不变，无报错状态变化（echo 未采集） |
| mark-quit | `hello world`、C-a → C-SPC → C-f×2 → C-g | 结束后 point=3、**mark=1**、active=false：C-g 只停用 region，mark 位置保留 |
| quit-no-mark | 空 buffer → C-g ×2 | 两次都回显 `Quit`；point/mark 不变 |
| minibuffer-quit | `C-x C-f` → 输入 → C-g | 取消 minibuffer，回显 `Quit`，buffer 不变 |
| buffers | A 设 mark → `C-x b foo RET` → 编辑并设 mark → `C-x b *spike* RET` | 切回 A 后 point / mark / active（1、3、true）保留；B 的状态独立；undo 只影响当前 buffer |
| mark-insert | `abcdef`、point=3 设 mark → 在 mark 处插入 `X` → `C-a` 插 `Y` → `C-e` 插 `Z` | 在 mark 处插入不移 mark（3→3）；在 mark 前插入移 1（3→4）；在 mark 后插入不移；文本 `YabXcdefZ` |
| mark-insert-undo | mark=3 → 在 point=1 插 `X` → undo → redo | undo 后 mark 回到 3（point=1）；redo 后 mark=4 |
| mark-delete-undo | mark=4 → `C-a` → `C-k` 删整行 → undo → redo | 删除把 mark 坍缩到 1；undo 恢复文本与 mark=4（point=1）；redo 再坍缩到 1 |
| mark-region-undo | mark=3 → `C-a` → `C-w` 删 `1..3` → undo | mark 在删除末端，坍缩到 1；undo 恢复 mark=3 |
| mark-undo-moved | 删除 `abc`（mark 4→1）→ point 移到 `def` 末尾设 mark=4 → `C-g` → undo | undo 插回 `abc` 后 mark 按文本位移到 7，**不**还原成 1：撤销只在 mark 仍在删除起点时恢复 marker |
| mark-buffers | A mark=4 → 切到 foo 编辑并设 mark → 切回 A 在 mark 前插入 → 回 foo undo | A 的 mark 跟随自己的编辑（4→5），B 的 mark/undo 不受影响 |
| undo-then-input | 见 `del-then-undo-then-type` | undo 后输入开新组，不复活旧内容 |

### undo / redo 的 point

- undo 恢复到该组开始前的 point（`undo-del` point=4、`undo-cd` point=1）。
- redo 不回到组末，而是回到组内**最早变更的起点**：`abc` 末尾输入 `zz` 后 redo，point=4（1-based）；
  `abc` 输入后再 undo/redo，point=1。spike 的 `Group::redo_point()` 按此规则实现。

### mark 的 marker 语义（批量探针 + command loop 复核）
mark 不是普通 offset，而是 marker（insertion type 为 nil）：

- 在 mark 位置插入：mark 不动（插入文本落在 mark 之后）；在 mark 前插入则整体后移，在 mark 后插入不动。
- 删除范围包含 mark：mark 坍缩到删除起点；mark 恰在删除末端也坍缩到起点。
- undo 是对文本变更的逆操作，但 Emacs 还会记录被删除范围内 marker 的调整：只有 mark 当前仍在删除起点时，undo 才恢复原来的偏移；
  如果删除后 mark 已被移到别处，undo 只按文本位移调整（`mark-undo-moved`：mark=7 而不是 1）。
- active 状态与 mark 位置独立；`C-g` 只停用 region，不移动 mark。
- `post-command-hook` 里看到的第一条编辑命令后的 active 可能尚未反映 transient-mark 的延迟停用（下一条命令才稳定），
  比较 active 时要看稳定后的命令，不能把这一拍的 True 当成语义差异。

### C-g 与前缀（重要修正）

`C-g` 单独按始终 `Quit`。`C-x C-g` 的结果取决于 pty 是否把两个字节合并进同一次 read：

- 分开到达（真实按键间隔，GUI 也是如此）→ 回显 `C-x C-g is undefined`，**mark 位置与 active 都保留**；
- 同一次 read 到达（`C-x C-g` 连发）→ `Quit` 且 deactivate region。

因此旧记录“按 region 是否有活动区分 Quit / undefined”是采集假象，不是 Emacs 语义。
真实 PGTK GUI（Hyprland + 真实按键）复核为 `C-x C-g is undefined`。
