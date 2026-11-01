# Emacs 31.1 对照基准（真实 command loop）

外部行为观察归档，不是 semacs 的通过记录。环境：GNU Emacs 31.1、`-Q`、fundamental-mode、transient-mark-mode 1、初始历史为空；使用 pty 驱动真实 command loop，GUI 复核另列。
采集和比较陷阱见 [README.md](README.md)；历史 dumps 的 index 1 是启动时 scratch 噪声，忽略。

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
| buffers | A 设 mark → `C-x b foo RET` → 编辑并设 mark → `C-x b *reference* RET` | 切回 A 后 point / mark / active（1、3、true）保留；B 的状态独立；undo 只影响当前 buffer |
| mark-insert | `abcdef`、point=3 设 mark → 在 mark 处插入 `X` → `C-a` 插 `Y` → `C-e` 插 `Z` | 在 mark 处插入不移 mark（3→3）；在 mark 前插入移 1（3→4）；在 mark 后插入不移；文本 `YabXcdefZ` |
| mark-insert-undo | mark=3 → 在 point=1 插 `X` → undo → redo | undo 后 mark 回到 3（point=1）；redo 后 mark=4 |
| mark-delete-undo | mark=4 → `C-a` → `C-k` 删整行 → undo → redo | 删除把 mark 坍缩到 1；undo 恢复文本与 mark=4（point=1）；redo 再坍缩到 1 |
| mark-region-undo | mark=3 → `C-a` → `C-w` 删 `1..3` → undo | mark 在删除末端，坍缩到 1；undo 恢复 mark=3 |
| mark-undo-moved | 删除 `abc`（mark 4→1）→ point 移到 `def` 末尾设 mark=4 → `C-g` → undo | undo 插回 `abc` 后 mark 按文本位移到 7，**不**还原成 1：撤销只在 mark 仍在删除起点时恢复 marker |
| mark-buffers | A mark=4 → 切到 foo 编辑并设 mark → 切回 A 在 mark 前插入 → 回 foo undo | A 的 mark 跟随自己的编辑（4→5），B 的 mark/undo 不受影响 |
| undo-then-input | 见 `del-then-undo-then-type` | undo 后输入开新组，不复活旧内容 |

### M1 历史补充参照

本批重新运行原有 typing / DEL / C-d / motion / RET / yank 分组、越过保存点与 marker 条件恢复的隔离 pty 用例，结果与上表一致。新增 pty 观察如下，point 均为 1-based：

| 用例 | 结果 |
|---|---|
| `m1-redo-backward-delete` | `abcDEF` 在 point=4 连续 DEL 两次，undo 恢复 point=4，redo 得到 `aDEF`、point=2 |
| `m1-redo-forward-delete` | `abcDEF` 在 point=2 连续 C-d 两次，undo / redo 均回 point=2 |
| `m1-redo-after-motion` | 在 `tail` 前输入 `abc` → undo → 成功 C-f → redo，仍恢复 `abctail`、point=1 |
| `m1-save-crossing` | 分三组输入 `ab`、RET、`cd` 后标记保存，undo 到 `ab\n` 与 `ab` 都为 dirty；redo 到最后保存边界为 clean。这不把所有“越过保存点”泛化为 clean |
| `m1-save-twice` | 输入 `ab` 后标记保存，再输入 `cd` 并标记保存；undo 到 `ab` 与 redo 到 `abcd` 均为 clean |

这里的保存操作仍是 `set-buffer-modified-p nil`，没有写磁盘。产品内部保存点入口同样不替代 M2 实际保存、磁盘字节与失败安全验收。
绑定导出保留 Emacs 默认 `undo-limit=160000`、`undo-strong-limit=240000`、`undo-outer-limit=nil` 的参照；不据此宣称自有线性历史与 Emacs GC / undo 环容量等价。

`m1-history.el` 的 6 项 ERT 通过，属于显式分界的函数 / interactive 调用，而非自动分组的 command loop。只读拒绝来自 interactive 检查，直接调用 `undo-only` 可以绕过；全局 marker 比较先停用 region，不将 Emacs 默认 undo-in-region 记为产品要求。

### M1 prompt 保护与字段参照

隔离 pty 的 `m1-minibuffer-prompt` / `m1-minibuffer-motion` / `m1-minibuffer-history` 重新运行；`Input: ` 的 prompt_end=8（1-based 字符位置）：

- 输入 `abc` 后 C-a 到 8，C-b 到 7，证明 prompt 只读不等于 point 不可进入。
- point=1 时 C-k 复制 `Input: ` 到 ring、point 到 8，但不删除 prompt；随后输入字段仍可正常编辑。
- 正文 kill `body` 后输入 `A`，进入 minibuffer C-y 得到 `body`，删除 `bo` 后 ring 为 `["bo", "body"]`；两次 undo 只恢复 / 撤销 minibuffer 编辑，不动正文的 `A`。退出参照读入后，正文 undo 仍独立工作。

`m1-fields.el` 的 7 项 ERT 使用显式的只读前缀 / field 属性，补足边界断言。C-a 受 field 约束，而 C-e 从 prompt 内仍到整条逻辑行末；C-k 则用所在字段的末端。yank 在只读文字处仍先设置 mark，再在非空插入时报告 text-read-only；它不同于整个 buffer 只读时的前置拒绝。

参照采集的 `text` 包含 prompt，`prompt_end` 与 point / mark 都是字符位置。纯函数 ERT 不能证明真实会话调度；pty 也不证明真实桌面 / IME。产品只实现本阶段已确认的固定只读前缀与 field 边界，不宣称实现完整文本属性或读入会话；参照的 C-g 结束会话不等于产品 M1 的编辑器 Quit。

### marker 观察的采集限制

上表 mark 场景经批量探针与 command loop 复核。`post-command-hook` 采集的第一条编辑命令后 active 可能尚未反映 transient-mark 的延迟停用，须比较稳定后的命令，不能把这一拍的 True 当成语义差异。

### C-g 与前缀（重要修正）

`C-g` 单独按始终 `Quit`。`C-x C-g` 的结果取决于 pty 是否把两个字节合并进同一次 read：

- 分开到达（真实按键间隔，GUI 也是如此）→ 回显 `C-x C-g is undefined`，**mark 位置与 active 都保留**；
- 同一次 read 到达（`C-x C-g` 连发）→ `Quit` 且 deactivate region。

因此旧记录“按 region 是否有活动区分 Quit / undefined”是采集假象，不是 Emacs 语义。
真实 PGTK GUI（Hyprland + 真实按键）复核为 `C-x C-g is undefined`。

### M1 kill / copy / yank

`m1-kill.el` 的 7 项直接函数 ERT 固定空 / inactive region、kill-line 空白与换行、copy 方向 / 命令链、只读 kill、yank mark、副作用和容量。Emacs 31.1 默认 `kill-ring-max=120`，不按旧版本常见的 60 推断。

隔离 pty 真实 command loop（`drive.py` 的三个 `m1-*` 用例，禁用 interprogram 与 copy blink delay）另行采集：

| 用例 | 稳定结果 |
|---|---|
| `m1-copy-kill-yank` | `ab\\ncd` 从 BOB 选中 `ab`，M-w 后 ring=[ab]；两次 C-k 后 ring=[\\ncd, ab]；C-y 恢复原文，point=6、mark=3、active=false（Emacs 1-based） |
| `m1-readonly-kill` | 只读 `abc\\ndef` 从 BOB 两次 C-k：文本与 modified 不变，先复制 `abc`，再追加换行；point 依次为 4、5，两次都报只读错误 |
| `m1-kill-copy-direction` | `abc\\ndef` 在 mark=1、point=3 时 C-k 得 ring=[c]；M-w 续接为 [cab]；再次 M-w 得 [ab, cab]；文本为 `ab\\ndef`，稳定后 active=false |

`post-command-hook` 在 copy / 首条 kill 后仍可能采到延迟停用之前的 active=true；如已有 marker 采集限制所述，不能把这拍当成稳定 active。产品 headless 对应命令链与共享语义测试独立断言即时最终状态；本记录不证明原生桌面 / IME 或历史分组已验收。

### M1 motion 直接函数参照

`m1-motion.el` 固定 Emacs 31.1 的词移动、逻辑行目标列 / 边界、TAB / Unicode 显示列、buffer 跳转的 mark，以及 batch selected window 的首次翻页。直接调用的下一条上下行命令显式设置 `last-command`，不声称经过真实 command loop。

- `beginning-of-buffer` / `end-of-buffer`：inactive 时在原 point 设置 inactive mark；active 时保留原 mark / active。
- `next-line` / `previous-line` 越界时仍先移到 EOB / BOB，再抛边界错误。
- `forward-word` 在 `éclair` 中包括组合符号，在 `🙂last` 中先停在 emoji 后；semacs 的字母 / 数字词规则与之不同，已登记为 F2。
- 首次向下翻页在固定 context=2、margin=0 时前进窗口正文行数减两行，point 到新页行首。未据 batch 的后续未 redisplay 状态固定连续翻页坐标。

本节是参照记录，不是产品、原生桌面 / IME 或 M1 性能验收。

### 键位查询

GNU Emacs 31.1 `-Q` / fundamental-mode 的绑定查询与缩进选项已固定于 [m1-bindings.tsv](m1-bindings.tsv)，M1 注册绑定由 `tests/keymap.rs` 逐项对照。`ESC ESC` 是前缀，`ESC ESC ESC` 为 `keyboard-escape-quit`；`C-x C-g`、`ESC C-g` 无绑定。

这是原始查询，不含 GUI 键名翻译：`<tab>` / `<return>` / `<backspace>` 的原始查询为 nil，不能据此判定真实 GUI 按键未绑定。基础缩进的固定选项与 26 组直接函数调用结果由 [m1-indent.el](m1-indent.el) 断言；字符运动、GUI Delete 与 `C-d` 的 region 差异、三次 ESC 的状态由 [m1-editing.el](m1-editing.el) 断言。不代表 semacs 命令执行或真实 command loop 验收通过。
