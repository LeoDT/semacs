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

### M2 `M-x` 读入会话参照

隔离 pty（`drive.py` 的 `m2-mx-*` 用例）在真实 command loop 中确认：

| 用例 | 稳定结果 |
|---|---|
| `m2-mx-complete` | `M-x ` 的 prompt_end=5（1-based）；输入 `for` 后两次 TAB 都不改变输入并打开候选列表 |
| `m2-mx-exit` | 输入 `forw` 后 RET 补全为 `forward-` 且**留在** minibuffer；随后输入 `z` 得到 `forward-z` |
| `m2-mx-unique` | 输入 `kill-regi` 后 RET 补全为 `kill-region`、退出并在正文执行（回显 `The mark is not set now, so there is no region`） |
| `m2-mx-unknown` | 输入 `nosuchcmd` 后 RET 留在 minibuffer；随后 C-g 退出并回显 `Quit` |
| `m2-mx-empty` | 空输入 RET 立即退出，正文回显 `` ‘’ is not a valid command name `` |
| `m2-mx-cancel` | 正文输入 `ab` 后 M-x 输入 `abc` 再 C-g：退出回显 `Quit`，正文文本、point 保持，随后输入 `z` 继续写正文 |

`for` 在 Emacs 完整 obarray 中已经是最长公共前缀（`fortune-*`、`format-*`、`forward-*` 共享 `for`），因此首个 TAB 不插入字符。semacs 的命令集不同，同一前缀通常会补到 `forward-`；补全结果差异来自候选集，不是补全算法。

`M-x` 使用 `require-match` 的 `completing-read`，因此 RET 走 must-match map。批量查询 `minibuffer-local-must-match-map` 得到 RET / C-m → `minibuffer-complete-and-exit`，TAB / C-i → `minibuffer-complete`，C-g → `abort-minibuffers`，ESC 为前缀，已固定于 [m2-minibuffer-bindings.tsv](m2-minibuffer-bindings.tsv)。

`minibuffer-complete-and-exit` 的源码与探针共同确认：空输入直接退出；精确匹配直接退出；否则先补全，只有补全后成为唯一精确匹配才在同一次 RET 退出；已在公共前缀且仍多候选时 RET 不产生可见消息；无候选时报告 `No match`。`simple.el` 的 `execute-extended-command` 使用 `` `%s' is not a valid command name ``，显示层按 text-quoting-style 呈现为弯引号，上述记录按显示结果书写。

候选集合在 Emacs 中是全部 `commandp` 符号；semacs 使用自己的注册表并排除会话命令，候选列表显示在 echo area 而不是 `*Completions*` buffer。这些差异属于产品已登记的实现边界，不是 Emacs 行为差异。本记录不证明真实 Wayland / IME 或 semacs 的 `M-x` 已通过验收。

### M2 文件 / buffer / 退出参照

[m2-files.el](m2-files.el) 用 6 项 ERT 固定文件与 buffer 查询语义；它只替换用户查询本身（`y-or-n-p` / `completing-read-default` / `read-buffer` / `read-file-name`），周围走真实 Emacs 代码。结果：

| 用例 | 稳定结果 |
|---|---|
| `m2-file-buffer-names-*` | `emacs -Q` 基线：第一个 `foo` 保持原名，第二个同名文件得到 `foo<目录名>` |
| `m2-find-file-reuses-*` | `find-file-noselect` 对符号链接和 `dir/../dir/file` 返回同一个 buffer，buffer 名保持第一次访问的 `real.txt` |
| `m2-saving-a-deleted-file-*` | 文件删除后 `save-buffer` 不调用 `yes-or-no-p`，直接重建文件并清除 modified；Emacs 按默认 final-newline 规则补了末尾换行 |
| `m2-write-file-overwrite-*` | `C-x C-w` 覆盖走 `y-or-n-p`；捕获到的提示参数是 `File ‘…’ exists; overwrite? `，`(y or n)` 由 `y-or-n-p` 自己追加 |
| `m2-kill-buffer-reads-*` | `C-x k` 读 buffer 名，默认当前 buffer，`require-match=t`；返回其他名字时 kill 该 buffer，当前 buffer 保留 |
| `m2-switch-to-buffer-*` | `read-buffer-to-switch` 排除当前 buffer，default 为 `other-buffer`，`require-match=confirm-after-completion` |

真实 pty command loop 另确认退出保存提示：dirty file buffer → `C-x C-c` 显示：

```
Save file /tmp/x? (y, n, !, ., q, M-~, C-r, C-f, d or C-h)
```

即 `save-some-buffers` 使用单键多选，`y` / `n` 本身就是有效答案，而不是 `yes-or-no-p` 的 `yes` / `no`。semacs 的 minibuffer 查询简化为 `(y or n)`，单键 y / n 即作答，不需要 RET；`!`、`.`、`q` 等附加动作保留已登记的 F2 简化。外部修改仍是 `yes-or-no-p` 语义，要求完整 `yes` / `no` 后 RET。

另在隔离 `emacs -Q -nw` pty 中直接调用查询函数，分别只发送 y / n 与 y / n / s（均不发送 RET）：`y-or-n-p` 返回 t / nil，`read-multiple-choice` 返回对应字符选择。此探针确认单键查询本身的交互，不冒充周围文件命令的完整 command loop，也不把 batch ERT 中被替换的查询当成按键证据。

这些记录是外部参照，不证明 semacs 已通过验收；未覆盖真实 Wayland / IME。

### F5：Magit 提交与历史参照

环境：GNU Emacs 31.1、Git 2.56.0、Magit `942d61d5de37c89a3a7c6cf693696b900b1d97b0`（`v4.7.1-74-g942d61d5`，调查时工作区干净）；直接从 checkout 加载 Magit，`emacs -Q --batch` 显式加载依赖，无日常 init / 桌面 / OS 注键。入口与隔离方式见 [README](README.md#f5magit-提交与历史参照)。参照测试源码 `f5-magit.el` 的 SHA-256 为 `87fee50e1fad131ac53c6a6828c67dc441674e177ddc70a483c5c0be94b990a5`。这是 F5 实施前的外部参照，不是 semacs 通过记录。

显式依赖包与主要源码 SHA-256（这些包并非 Magit checkout 锁定的统一依赖快照）：

| 包 | 主要源码 SHA-256 |
|---|---|
| compat-31.0.0.2 / compat.el | `e7c2ddc4b6d7af6e6b038c86a0d11bf23907d5b6018c406683bcef41d1518ce3` |
| cond-let-20260817.452 / cond-let.el | `667438802590ac16783d088ff7b39eb9a5a0379d5fe628d609d1d37b3a0741fa` |
| dash-20260221.1346 / dash.el | `ce8043bfcfe64bfe69a411ee29e4c704213abd93aaa9a6da8b6791d3110d7f48` |
| llama-20260601.1455 / llama.el | `44b1790e14a84d69c03889f8bd9a7975406bf996efac101a76477a0a1331f97b` |
| transient-20260825.819 / transient.el | `cac5353bffee2f06b1961da6616dd5ecc2e61b79901c765410efee02b06034e2` |
| with-editor-20260731.2234 / with-editor.el | `6ea16f15b201ab1c26b04bd4df514f915f58787786f2d58d802de5ae26dfc106` |

扩展后的 13 项 ERT 全部符合预期，无失败。观察范围：

| 用例 | 结果与证据层次 |
|---|---|
| amend 没有 staged | 真实 Magit 函数仍调用 `commit --amend --verbose`；启动器被捕获，不证明完整异步提交。 |
| reword | 真实函数传 `--amend --only --allow-empty`；捕获的启动器转而运行真实 Git 和 fixture editor，消息变为 reworded，HEAD tree 不变、原 staged index 不变。不是 semacs helper 或 Magit async editor 验收。 |
| staged fixup / squash | 真实函数先创建待选回调及 staged companion；显式调用回调后，启动参数分别包含 `--fixup=OID` / `--squash=OID` 和 `--no-edit`，无 rebase；选择 UI / 启动器被捕获。 |
| fixup 只有 unstaged | 真实谓词及共享 assert 路径触发 unstaged 预览与 `Nothing staged. Commit all uncommitted changes?` 查询；答 yes 后才选目标，并带 `--all` 启动。`strict` 并不排除该分支。查询 / 显示 / 启动器被捕获。 |
| fixup 拒绝 / 无改动 | 拒绝 `--all` 不选目标、不启动；没有任何改动则 user-error。 |
| 选择键位 | map 查询：`C-c C-c`、`.`、`e` 为 pick，`q` 为 quit，`p` 不是 pick；不是实际按键驱动。 |
| 日志 | 默认 graph / decorate / 256；真实 limit 函数将 256 加倍为 512，再减半为 256（刷新被替换）；当前分支为 main，detached 时为 HEAD（setup 被捕获）。 |
| 已发布 amend | fixture 的 `origin/master` 包含 HEAD，真实检测走 `amend-published`；替换确认函数拒绝后不会启动。未覆盖真实确认按键。 |
| revision 命令 | 捕获真实入口使用 `show -p --format=`，没有强制 first-parent。 |
| root / merge revision | 真实 batch revision buffer 显示 root 的 `+base`；两父提交的 resolution fixture 由真实 Git show 产生 combined，真实 Magit buffer 含 `@@@` 及 `++resolved`。不是屏幕呈现 / 文件版本访问覆盖。 |

尚未观察：真实 command loop / transient 与选择界面按键、GUI / IME、发布及 merge-range 多步查询、完整 hooks / 签名 / editor 失败生命周期、combined 的各 parent 版本访问。以上源码静态入口和实施待决范围见 [F5 调研](../../docs/work/f5-history-commit.md)，不以本记录替代产品契约或批准差异。

## M3：鼠标选区与文件读入预填参照

环境：GNU Emacs 31.1、`emacs -Q -nw`、24x80 pty、`TERM=xterm-256color`、fundamental-mode、transient mark，按 [README](README.md) 的 pty 驱动方式启动，工作目录为仓库根。`m3-mouse-*` 用例在 `baseline.el` 中额外启用 `xterm-mouse-mode`，并以 xterm SGR 序列注入鼠标（`ESC[<b;col;rowM` 按下 / 移动，`m` 释放；button 0 = 左键，32 = 按住左键移动）。观察值取 `baseline.el` 的 post-command-hook dump，其中新增 `start` / `end` 记录 `window-start` / `window-end`。

```sh
python3 tests/emacs/drive.py m3-mouse-click-drag m3-mouse-autoscroll m3-mouse-below-eob m3-read-find-file m3-read-write-file
```

运行前提：`insert-default-directory` = `t`、`find-file-run-dired` = `t`、`mouse-scroll-delay` = 0.25、`mouse-scroll-min-lines` = 1、`mouse-drag-copy-region` = `nil`、`mouse-shift-adjust-mode` = `nil`（`S-down-mouse-1` 仍是 `mouse-appearance-menu`）；以上取值由 `emacs -Q --batch` 打印确认。`m3-mouse-*` 文本为 100 行 `lNN aaaa\n`（900 字节），`m3-mouse-below-eob` 为 3 行同名格式（27 字节，point-max 为 28），point 从 buffer 起点开始。24 行终端中 row1 是 menu bar，row2–22 是正文，row23 是 mode line，row24 是 echo area。

| 用例 | 操作 | 结果 |
|---|---|---|
| m3-mouse-click-drag | 单击 row3 col6 | 按下后 point=mark=15、active；释放后 point=15、mark=15、**region 停用** |
| m3-mouse-click-drag | 再单击 row6 col6 | 按下 point=mark=42、active；释放后 point=42、**mark=15（恢复点击前的 mark）**、region 停用 |
| m3-mouse-click-drag | 按下 row9 col5 → 移动 row8 col9 → 移动 row11 col9 → 释放 | point 68→63→90，mark 固定 68，region 全程 active，释放后仍 active |
| m3-mouse-autoscroll | 按下 row4 col5 → 移动 row22 col5（24 行终端正文末行） | point=185；window-start 仍为 1，不滚动 |
| m3-mouse-autoscroll | 移动到 row30 col5（窗口下边界外） | window-start 1→217，point=397；再次相同移动 → start 433、point=613 |
| m3-mouse-autoscroll | 在窗口外释放 | window-start 最终 505，point=685；mark 仍 23、region 仍 active |
| m3-mouse-autoscroll | 滚动后按下 row3 col4，再向右移到 col200 两次 | point 跟随窗口内位置（517）；col200 在 pty 外，point 与 window-start 均不变（**横向离开不滚动**） |
| m3-mouse-below-eob | 单击 row10 col5 / row20 col5（最后一行以下空白） | 按下 point=28=**point-max**、mark=28、active；释放 point=28、mark=28、region 停用 |
| m3-mouse-below-eob | 单击 row3 col40（第 2 行文本右侧空白） | 按下 point=18=行尾（换行之前），与最后一行以下的空白不同 |
| m3-mouse-below-eob | 按下 row10 col5 → 移动 row2 col5 → 释放 | point 28→5，mark 固定 28，region 全程 active，释放后仍 active |
| m3-mouse-below-eob | 按下 row2 col5 → 移动 row12 col5 → 释放 | point 5→28（空白处夹到 point-max），mark 5，region active |
| m3-read-find-file | `C-x C-f` | minibuffer 为 `Find file: ~/src/semacs/`，输入即缩写默认目录 |
| m3-read-find-file | 未编辑 `RET` | 打开 dired（`dired-mode` 列出仓库目录） |
| m3-read-write-file | `C-x C-w` | minibuffer 为 `Write file: ~/src/semacs/` |

补充观察（同一环境，单独 pty 运行）：

- 短 buffer 下单击 row19–row22 均为 point-max，单击 row23 / row24（mode line / echo area）point、mark、region 均不变；100 行 buffer 用 `M->` 滚到末尾后（start=739、end=901），单击正文最后一行的 row19 得到 896，row20–22 均为 901=point-max。
- 上表 row10 / row20 的两次单击中，释放前的 mark 都是上一次执行后的 mark，释放后按 `pop-mark` 回到该值；该恢复行为是 semacs 未实现的上游分支，见保真度契约的已批准差异。

- `find-file-run-dired` 设为 `nil` 时，`(find-file-noselect "…/dir/")` 报 `… is a directory`（源码 `files.el` 对目录的错误分支）；有 dired 时 `find-file` 转 dired，因此“目录答案”在上游没有“保留会话并提示”的分支。
- 无路径 buffer 的 `C-x C-w` 未编辑 `RET` 使用 `default-directory` + buffer 名：当时 buffer 名 `semacs`，写入 `…/semacs` 并回显 `Wrote …`；与 `write-file` interactive spec 的 `(expand-file-name (file-name-nondirectory (buffer-name)) default-directory)` 一致。

补充源码 / 选项核对（未逐项运行）：

- `down-mouse-1` 走 `mouse-drag-region`；双击选词、三击选整行（含行尾换行），`mouse-skip-word` 还区分空白、标点与 symbol。
- `mouse-drag-copy-region` 默认 `nil`；`mouse-shift-adjust-mode` 默认关闭，默认 `S-down-mouse-1` 是 `mouse-appearance-menu`。

观察边界：鼠标事件经 pty 注入，只有 24x80 字符终端坐标与上述固定选项，不覆盖真实桌面指针精度、双击 / 三击 / Shift / 右键、拖动移动文本或 dired 交互；也不覆盖 GUI 下的 fringe / 滚动条或正文区以外空白。本记录也不证明 semacs 的宿主鼠标交互或读入实现通过。semacs 的已批准差异（单击不恢复旧 mark、缺少 dired、滚动按像素换算行）见 [保真度契约](../../docs/emacs-fidelity.md#鼠标选区)。
