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
- 完整 keymap / 指针注入还需要：`wayland-scanner`、`gcc`、`pkg-config`、
  `wayland-client` 与 `xkbcommon` 开发文件（`scripts/vkbd`、`scripts/vptr` 首次运行自动构建）。
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

对比只保留带行为的行：`KEY RESOLVE CMD ECHO CHANGE DUMP DUMP2 BUFFERS SYNC REFUSE CANCEL CLOSE ACTIVATION
FAKE-PREEDIT OWNEDIT OWNUNDO REDO REPLAY MINI PREEDIT PREFIX TARGET`，
并丢弃 harness 噪声（虚拟键盘导致的 `WINDOW active` 抖动、高频 `accepts_text_input` 轮询、pid）。

### 加 case

1. `cases/<name>.sh`：用 `run.sh` 提供辅助函数。`keys a` 输入字符，
   `keys -M ctrl -k x -m ctrl` 发 `C-x`（wtype，旧 case 使用）；`vk shift+a` / `vk ctrl+question`
   用真实 evdev keymap 发 Shift / 死键 / AltGr；`mouse drag X1 Y1 X2 Y2` 发绝对指针拖选。
   多键序列必须一次发完，中间不要插探针键（会吃掉 pending 前缀）。
   需要在同一批次里保持合成 marked text 时，必须放在**同一个 `vk` 调用**里
   （虚拟键盘 leave 会让 GPUI 删掉 marked text）。
   真实会话的 `Ctrl-SPC` 会被 fcitx5 当作激活快捷键；spike 的 F7 probe 走同一条 `set-mark-command`。
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
| `s2-quit` | `C-g` / 前缀 + `C-g` 的 echo、mark 与 active 矩阵（与 GUI Emacs 一致） | S2 |
| `s3-own-undo` | 自建 undo 的分组：连续输入 / 移动打断 / 连续删除 / 连续插入 / redo | S3 |
| `s3-kill` | `C-k` 两次（第二次在 EOB 报 `End of buffer` 且不建 undo 组）、undo 到底 | S3 |
| `s3-kill-yank` | kill / yank 各自成组；yank 设 mark 并回显 `Mark set` | S3 |
| `s3-dirty` | 保存 → 编辑 → undo 回到保存点，dirty 变回 false | S3 |
| `s3-buffers` | 两个 buffer 的 text / point / mark / active / undo 隔离；切换是 undo 边界 | S3/S4 |
| `s3-mark-follow` | mark 是 marker：插入在 mark 前 / 处 / 后时的位置变化 | S3/S4 |
| `s3-mark-undo` | 插入 undo / redo 时 mark 跟随文本，不按组快照回滚 | S3/S4 |
| `s3-mark-delete` | 删除覆盖 mark 时坍缩到起点；undo 恢复、redo 再坍缩 | S3/S4 |
| `s3-mark-moved` | 删除后 mark 移走，undo 只按文本位移调整（Emacs marker 规则） | S3/S4 |
| `s3-mark-buffers` | A/B 的 mark 与编辑 / undo 互不影响 | S3/S4 |
| `s3-undo-then-input` | undo 后立即输入开新组并清空 redo | S3 |
| `s4-observe` | 平台文本输入经适配器可见（范围 + 内容）；预编辑；pending 前缀期间字符按序列解析、不插入 | S4 |
| `s4-mark-ime` | IME 提交按插入调整 mark，undo 恢复；重复预编辑 / 提交由适配器同一入口处理 | S4 |
| `s4-ime-barrier` | D17 策略分支：组合中 save / 前缀 / 切换被拒绝，组合结束后恢复 | S4 |
| `s4-ime-close` | D17 / D19 非命令入口：组合中关闭先取消预编辑再允许关闭，不保存、不变 dirty | S4 |
| `s4-mouse` | 虚拟指针正反向拖选 → point / mark / active 导入；单击保留 mark | S4 |
| `s4-readonly` | 只读目标拒绝输入并回显 `Buffer is read-only`，motion 仍工作 | S1/S4 |
| `s5-raw` | 原始键名与 `key_char` 分类：RET/`C-m`、TAB/`C-i`、DEL、ESC、箭头、M-f、AltGr、死键 | S5 |
| `s5-shift` | wtype 单层 keymap 的限制记录（Shift 被吞、C-? 退化成 C-/）；已被 `s5-keymap` 取代 | S5 |
| `s5-keymap` | 真实 evdev keymap：Shift 字符、`C-?` vs `C-/`、死键组合、AltGr、undo 组 | S5 |

## 真实会话 case（opt-in，会抢焦点）

`real-cases/` 在**用户真实的 Hyprland 会话**里跑，用于真 fcitx5 / 真合成器路径：

```bash
SPIKE_REAL=1 scripts/run-real.sh s4-ime     # 运行并打印日志
SPIKE_REAL=1 scripts/check-real.sh --all    # 与 real-cases/*.expected 对比
```

安全约束：

- 必须显式 `SPIKE_REAL=1`；否则脚本拒绝运行。
- 每次注键前用 `hyprctl` 校验焦点仍是 lab 窗口，焦点丢失立即中止，绝不把键发到终端。
- 保存并在退出时恢复 fcitx5 的输入法、激活状态与原 workspace。
- 焦点 / 关闭使用 Hyprland 0.56 的 Lua dispatcher（`hl.dsp.focus(...)` / `hl.dsp.window.close()`）；
  `run-real.sh` 的 `focus_window` / `focus_workspace` 带旧语法回退。
- `real-cases/*.expected` 绑定本机环境（Hyprland + fcitx5 pinyin + us 布局），换机器/升级后需人工审阅并
  `UPDATE=1` 重写；preedit 更新噪声在 `check-real.sh` 中被过滤，commit / cancel / text / undo 仍断言。

| real case | 验证什么 |
|---|---|
| `s4-ime` | 真实 fcitx5 预编辑 / 空格提交 / Escape 取消 / 提交后 undo（不再 panic）/ 预编辑不 dirty |
| `s4-ime-save` | D17：组合中保存被拒绝且组合保留；取消后同一命令保存已完成文本 |
| `s4-ime-close` | D17 / D19 非命令入口：组合中关闭先取消预编辑再关闭，不保存、不变 dirty |
| `s4-ime-focus` | D17 / D19 非命令入口：组合中切 workspace，平台取消组合，文本 / undo / dirty 不变；恢复后无迟到污染 |
| `s4-ime-mark` | 真实 fcitx5 空格提交后 mark 跟随，undo 恢复（F7 probe 避开 `Ctrl-SPC`） |
| `s5-real-keymap` | 真实 Hyprland 下的 Shift / `C-?` / 死键 / AltGr 分类（与 headless 结果对照） |

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

- **真实 IME**：`s4-observe` / `s4-ime-barrier` 用适配器或假标志模拟组合；真实 fcitx5 路径只在
  `real-cases/s4-ime*` 验证，且依赖本机环境。
- **鼠标**：`vptr` 是 wlr virtual pointer（协议等价于物理指针），不是物理鼠标；与 IME / caret 同步的交错未测。
- **Shift / `C-?` / 死键**：`s5-keymap` 上传与合成器相同的 keymap，协议层等价；不是字面硬件键盘。
- **D17 / D19 非命令入口**：关闭与焦点切走已在真实 Hyprland + fcitx5 验证：未提交预编辑按取消处理，
  关闭 / 失焦不保存、不变 dirty。WM 强退与进程崩溃未测。
- **mark 跟随**：`s3-mark-*` 已与 Emacs 31.1 对照；`s4-mark-ime` 是适配器驱动的合成提交，
  真实 fcitx5 路径由 `real-cases/s4-ime-mark` 验证。
- **redo point**：已对齐 Emacs：redo 回到组内最早变更起点，`Group::redo_point()` 有单测；`s3-own-undo` / `s3-mark-undo` 断言。
- **多 buffer / 多窗口 / 性能**：只做了两个 buffer 的状态隔离；没有多窗口，也没有性能测量。

产品级结论以 `docs/` 为准；本目录的 `EVIDENCE.md` 是过程证据与代码依据。
