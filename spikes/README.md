# spikes —— 验证工程（不是产品代码）

S1–S5 前置门禁的验证工程与 Emacs 对照基准。产品设计见 [../docs/](../docs/)，
已完成结论与证据见 [EVIDENCE.md](EVIDENCE.md)。

## 生命周期

- 这些代码服务于 spike 阶段：证明机制可行、把与 Emacs 的行为差异固定成可复现的对照。
- **M1 有等价实现并通过同样 scenario 后，本目录应当删除**；在此之前它是回归与对照的唯一可执行依据。
- 产品代码不得依赖本目录（独立的 cargo 工程，只依赖 crates.io 的 `gpui-kit`）。

```
lab/          跑在私有 headless sway 里的 spike 应用 + case 脚本 + 期望输出
reference/    Emacs 31.1 真实 command loop 对照基准（pty 驱动）
EVIDENCE.md   已验证的事实、代码依据、API 缺口、harness 坑
```

## lab

### 环境

- Linux + Wayland 优先；case 在**私有 headless sway** 里跑，不碰当前桌面会话。
- 依赖：`sway`（1.12 验证过）、`wtype`、`grim`、`mesa`（软件渲染兜底）。
- 沙箱里若没有 `/dev/dri`，必须 `export LIBGL_ALWAYS_SOFTWARE=1`，否则 EGL/ZINK 初始化失败、窗口画不出来。
- 首次运行会构建 `gpui-kit = "0.8.1"`（crates.io）；`Cargo.lock` 已提交，保证可复现。

### 跑 case

```bash
export LIBGL_ALWAYS_SOFTWARE=1
cd spikes/lab

scripts/run.sh s3-kill            # 跑单个 case，打印日志与截图路径
scripts/check.sh s3-kill          # 跑并对比 cases/s3-kill.expected
scripts/check.sh --all            # 全部 case
UPDATE=1 scripts/check.sh s3-kill # 行为变化后重写期望值（人工审阅 diff 再提交）
```

产物：

```
log/<case>.log         lab 自己的日志（含噪声）
log/<case>.normalized  过滤后的日志（check.sh 比对用）
log/<case>.png         截图
log/<case>.app.log     应用 stderr（渲染/协议问题看这里）
```

对比只保留带行为的行：`KEY RRESOLVE CMD ECHO CHANGE DUMP DUMP2 OWNEDIT OWNUNDO REDO REPLAY MINI PREEDIT PREFIX TARGET`，
并丢弃 harness 噪声（虚拟键盘导致的 `WINDOW active` 抖动、高频 `accepts_text_input` 轮询、pid）。

### 加 case

1. `cases/<name>.sh`：用 `run.sh` 提供的 `keys` / `shot` 辅助函数发按键。`keys a` 输入字符，
   `keys -M ctrl -k x -m ctrl` 发 `C-x`。多键序列必须一次发完，中间不要插探针键（会吃掉 pending 前缀）。
2. 需要初始状态时写 `cases/<name>.env`（会被 source，例如 `export SPIKE_INITIAL=$'ab\ncd'`、`export SPIKE_MODE=log`、`export SPIKE_READONLY=1`）。
3. `UPDATE=1 scripts/check.sh <name>` 生成期望值，**逐行审阅**后再提交。
4. 涉及 Emacs 参照的 case，同时在 `reference/drive.py` 的 `CASES` 里加一条并记录到 `reference/RESULTS.md`。

### case 清单

| case | 验证什么 | 对应 |
|---|---|---|
| `s1-consumed` | 被消费的命令键（`C-a`/`C-x`/`C-c`/`C-v`/TAB/RET/ESC）不触发底座 action、不插字符、不移动 point | S1 |
| `s1-unconsumed` | 对照：不消费时 TAB 插空格、RET 换行（证明拦截是被消费键的关键） | S1 |
| `s2-prefix` | `C-x` 前缀无超时；`C-x C-s`、`C-x u` 命中；`C-x C-g` 放弃前缀 | S2 |
| `s2-minibuffer` | 最小 minibuffer 走公共 EditTarget：prompt、输入、`C-a`、`C-g` 取消、RET 确认、焦点恢复 | S2 |
| `s2-killring` | 正文与 minibuffer 共享 kill ring；非 kill 命令打断追加链 | S2 |
| `s3-own-undo` | 自建 undo 的分组：连续输入 / 移动打断 / 连续删除 / 连续插入 / redo | S3 |
| `s3-kill` | `C-k` 两次（第二次在 EOB 报 `End of buffer` 且不建 undo 组）、undo 到底 | S3 |
| `s3-kill-yank` | kill / yank 各自成组；yank 设 mark 并回显 `Mark set` | S3 |
| `s3-dirty` | 保存 → 编辑 → undo 回到保存点，dirty 变回 false | S3 |
| `s4-observe` | 平台文本输入经适配器可见（范围 + 内容）；预编辑 / 提交；前缀期间字符不插入 | S4 |
| `s4-readonly` | 只读目标拒绝输入并回显 `Buffer is read-only`，motion 仍工作 | S1/S4 |
| `s5-raw` | 原始键名与 `key_char` 分类：RET/`C-m`、TAB/`C-i`、DEL、ESC、箭头、M-f、AltGr、死键 | S5 |
| `s5-shift` | Shift 与 `C-?` 在虚拟键盘下的观测（记录为 layer 3 待验证） | S5 |

## reference（Emacs 对照）

固定基准：GNU Emacs 31.1、`emacs -Q`、fundamental-mode、transient-mark-mode 开。
驱动方式是在 pty 里跑**真实 command loop**（不是 batch 里直接调函数），`post-command-hook`
每条命令后把 text / point / mark / active / modified / echo 文案追加到 jsonl。

```bash
cd spikes/reference
python3 drive.py undo-del dirty kill2      # 跑若干 case，打印状态序列
python3 drive.py                           # 无参数：列出可用 case（见 CASES 表）
emacs -Q --batch -l export-keys.el         # 导出 v1 默认键位的命令名基准
```

结果记录在 [RESULTS.md](reference/RESULTS.md)。两次踩过的坑已在脚本里修掉，改动时别退回去：

- 用 `append-to-file` 写状态会污染 echo area（消息变成 "Added to ..."）→ 必须 `write-region` + `inhibit-message`。
- 启动时 *scratch* 会多执行一条命令，导致 dump 序列错位 → setup 用 `window-setup-hook`。
- 初始文本的插入会进 undo 历史 → setup 末尾清空 `buffer-undo-list` 并 `set-buffer-modified-p nil`。
- 初始 point 用 `goto-char point-max`（Emacs 的 point 是 1-based，对比时注意换算）。
- 需要"保存点"或"只读"时用 `C-c m` / `C-c r` 这类临时代码绑定，不要用 batch 直接调函数。

## 证据边界（这些 case 不证明什么）

- **真实 IME**：`s4-observe` 的预编辑是自驱的适配器调用，不是 Wayland IME；需要真机 fcitx5 验证。
- **鼠标**：layer 2 没有指针设备（`wtype` 不能移指针），拖选导入 point/mark 未验证。
- **Shift / `C-?`**：`wtype` 的虚拟键盘 keymap 会吞 shift，`C-?` 与 `C-/` 无法区分（`s5-shift` 记录的就是这个限制）。
- **多 buffer / 多窗口 / 性能**：lab 只有单 buffer、单窗口，也没有性能测量。

产品级结论以 `docs/` 为准；本目录的 `EVIDENCE.md` 是过程证据与代码依据。
