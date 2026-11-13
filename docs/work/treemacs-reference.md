# Treemacs 参照：文本树与文件浏览

> 本文是方向确认后的上游参照调查，**不是 semacs 行为契约或实施规格**。首版范围、键位、布局与取舍仍待确认；当前计划见 [实施计划](../plan.md)，逐项运行结果与证据边界见 [F6 观察记录](../../tests/emacs/RESULTS.md#f6treemacs-文件夹树参照)。

## 来源与证据边界

- 本地安装：`/home/leodt/.emacs.d/elpa/treemacs-20251226.1307`，包版本 `20251226.1307`，上游 commit `2ab5a3c89fa01bbbd99de9b8986908b2bc5a7b49`（`https://github.com/Alexander-Miller/treemacs`）。
- 用户日常配置仅作参考，不是参照环境：`treemacs-indentation` 为 1、`treemacs-move-files-by-mouse-dragging` 为 nil、`treemacs-filewatch-mode` 为 1、`treemacs-git-mode` 为 `simple`、`C-c t` 绑定 `treemacs-select-window`。
- 参照环境为 `emacs -Q` + 上述固定 checkout：batch ERT（`tests/emacs/f6-treemacs.el`，31 项）+ 真实 command loop（`tests/emacs/f6-drive.py`，40x100 tty）+ 键位导出（`tests/emacs/f6-bindings.tsv`）。
- 本文最初的结论来自源码静读；首次运行后凡与运行结果冲突处都已按运行结果改写，并标出静读错误。仍未运行的部分（GUI 呈现、鼠标、workspaces、peek、文件管理、远程）明确保留为未观察。

## 上游行为（实测）

### 窗口与入口

- 专用 side window，默认左侧、`treemacs-width` 35（选项是正文宽度，tty 里窗口总宽 = 正文 + 1）。窗口参数：`window-side` = left、`window-slot` = 0（创建时 -1，由 `window-configuration-change-hook` 归一）、`no-delete-other-windows` = t；树 buffer 里 `window-size-fixed` = width。
- 保护只针对 `delete-other-windows`：**在树里执行 `C-x 1` 会报错 `Cannot make side window the only window`**，树永远不能成为唯一窗口；而 `delete-window` / `C-x 0` 可以正常关掉它（静读文档说的“不易关闭”只对 `C-x 1` 成立）。
- `C-x o` 可以在普通窗口与树之间来回切换，side window 不被跳过。
- `treemacs` 三态：可见→隐藏（bury，buffer 存活）、已存在→显示并选中、不存在→创建（工作区为空时询问首个项目根）。
- 消失后重新显示会重新设置 `no-delete-other-windows`（靠 `window-configuration-change-hook`，只有真实 redisplay 才生效）。
- `q` = `treemacs-quit`（bury），`Q` = `treemacs-kill-buffer`（kill），`treemacs-select-window` 已在树中时按 `move-back` 回到上一个窗口。
- `treemacs-find-file` 定位并展开当前文件的节点；树可见或隐藏都不抢焦点。当前 buffer 无文件时带前缀参数改为读入路径。

### 树与节点

- 普通文本 buffer + button / text property，不是 GUI 控件树；根行是项目目录名，目录行前缀折叠时 `+ `、展开时 `- `，文件行前缀一个空格，每层缩进 `treemacs-indentation`（默认 2 个空格）。
- 目录在前、文件在后，各自按 ASCII 区分大小写排序（默认 alphabetic-asc）；`s` 弹出 12 项排序方法的 completing-read（不是循环），`C-u` / `C-u C-u` 只是临时重排当前目录 / 整个视图。
- `treemacs-collapse-dirs` 默认 3（python3 可用时）：只含一个子目录且不含文件的链条合并成一行，到含文件的目录为止。
- `TAB`：root / 目录开合，文件展开 tag 段（无 tag 时回显 `No tags found for …`）；`RET`：root / 目录开合，文件走默认访问动作，tag 跳转；`h` / `l` 是 COLLAPSE / RET 的别名。
- 目录读取与刷新保留可匹配的折叠状态与 point；`g` / `r` 刷新、`s` 排序、`<` / `>` / `=` / `w` 调整宽度（`=` 会把最长可见行宽度写回选项）。
- `n` / `p` = `treemacs-next-line` / `previous-line`（按 button 走行，受 wrap-around 影响）；`M-n` / `M-p` = `treemacs-next-neighbour` / `previous-neighbour`（同层）；**`C-n` / `C-p` 仍是 `next-line` / `previous-line`**。
- 默认键位共 91 条（含 `o` / `t` / `y` / `C-c C-p` / `C-c C-w` 前缀），清单见 [f6-bindings.tsv](../../tests/emacs/f6-bindings.tsv)。

### 默认显示策略

- dotfiles **默认显示**（`treemacs-show-hidden-files` = t），`t h` 切换；`.git` / `.jj` 目录始终隐藏。
- gitignored 文件**默认显示**，`t i`（`treemacs-hide-gitignored-files-mode`，默认关闭）在某种 `treemacs-git-mode` 下才会隐藏它们。
- 加载时自动开启：`treemacs-follow-mode`、`treemacs-filewatch-mode`、`treemacs-fringe-indicator-mode`；有 git + python3 时 `treemacs-git-mode` = `deferred`。静读文档把这些当成纯可选能力，实际是默认开启。
- follow 生效方式：在树旁边打开文件后，空闲 `treemacs-file-follow-delay`（0.2 s）内树展开对应目录并把 point 移到该文件，焦点不动。

### 项目根

- `treemacs--find-user-project-functions` = (builtin, 当前目录)：先 projectile（若安装）/ project.el，取到就用；否则**回退为当前 buffer 自己的目录**，不向上搜索标记文件。
- `.git` 目录和 `.git` 文件（worktree）都能被识别为仓库根；无 VCS 时根就是打开文件所在目录。因此“手动目录”不是额外入口，而是兜底行为。

### 访问目标

- 默认动作 `treemacs-default-visit-action` = `treemacs-visit-node-no-split`：在树旁边的窗口打开（已打开则复用），选中它，树保留且不新增 split。
- `C-u RET` 打开后焦点回到树；`C-u C-u RET` 打开后删除树窗口（转入 exists 状态）。
- `o` 前缀提供 no-split / 水平 split / 垂直 split / 最近使用窗口 / ace-window / 外部程序 / 访问后关闭树。

## 静读错误清单（已按运行结果更正）

1. “side window 不会被 `delete-other-windows` 轻易关闭” → 只对 `C-x 1` 成立；`delete-window` 能关，树里 `C-x 1` 直接报错。
2. “follow / filewatch / git 注解是可选能力” → 加载即默认开启（git 注解为 deferred）。
3. 未提及 `treemacs-collapse-dirs`（默认 3，单链目录合并）。
4. 未明确 dotfiles 默认显示、`.git` / `.jj` 恒隐藏、gitignored 默认显示。
5. “`s` 刷新排序” → `s` 是排序方法选择器，刷新是 `g` / `r`。
6. 未记录 `M-n` / `M-p` 与 `n` / `p` 的语义差别，以及 `C-n` / `C-p` 未被改绑。
7. 未记录 `RET` 的默认目标是“树旁边的窗口”以及两个前缀参数的差别。
8. 根识别不是 semacs 式的“只认 `.git`”，而是 project.el 优先、当前目录兜底。

## semacs 方向（side window 已确认，其余待细化）

- 形态：独立文本 buffer，放在专用左侧 side window；side window 独立于普通 split，不另建 GUI 树控件。复用现有 section 树、可见行映射、mode 局部键位、focus / 临时 window 语义。
- 根：先支持当前项目根的单根树；根来源按上游（project.el 优先 + 当前目录兜底）。
- 节点：root / 目录 / 文件；目录懒加载，目录在前、文件在后；折叠状态归 buffer，point / scroll 归 view；刷新保留可匹配的折叠与位置。
- 首版不做：文件增删改 / 移动、workspaces、图标、tags / imenu、filewatch、peek、git 注解、持久化、远程 / Tramp。follow 上游默认开启，首版照搬（保留 `t f` 开关）；其余不做的能力已在保真度契约末尾登记。
- 失败与边界：不可读目录、符号链接、项目根失效、刷新时节点消失、异步目录读取迟到、树 buffer 被另存或 kill；沿用现有目标 / 版本隔离与只读生成 buffer 规则。
- 性能：不在首版承诺新增预算；目录读取必须有界、可取消，避免递归扫描整棵树阻塞前台。

## 已确认（2026-11-13）

用户确认：上游 treemacs 的实测行为**全部照搬**，上述待确认的产品取舍全部按上游取值——侧窗保护语义、键位（含 `n` / `p` / `M-n` / `M-p` 与保持不动的 `C-n` / `C-p`）、根识别（project.el 优先 + 当前目录兜底）、dotfiles / gitignored 默认显示、follow 默认开启与当前文件定位、访问目标与前缀参数。上游没有的额外限制、确认或快照检查不引入。

有效契约见 [保真度：阶段 F6](../emacs-fidelity.md#阶段-f6文件夹-tree-viewer)；首版不提供的上游能力（排序选择器、git 注解与 `t i`、filewatch、图标、peek、文件管理、workspaces、持久化等）也在该节末尾登记，属于已确认的首版范围缺口，不是对上游行为的改写。实施顺序见 [实施计划](../plan.md)。
