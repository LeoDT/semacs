# Treemacs 参照：文本树与文件浏览

> 本文是方向确认后的上游参照调查，**不是 semacs 行为契约或实施规格**。首版范围、键位、布局与取舍仍待确认；当前计划见 [实施计划](../plan.md)。

## 来源与证据边界

- 本地安装：`/home/leodt/.emacs.d/elpa/treemacs-20251226.1307`，包版本 `20251226.1307`，上游 commit `2ab5a3c89fa01bbbd99de9b8986908b2bc5a7b49`（`https://github.com/Alexander-Miller/treemacs`）。
- 用户日常配置仅作参考，不是验收环境：`treemacs-indentation` 为 1、`treemacs-move-files-by-mouse-dragging` 为 nil、`treemacs-filewatch-mode` 为 1、`treemacs-git-mode` 为 `simple`、`C-c t` 绑定 `treemacs-select-window`。
- 静态阅读 `treemacs.el`、`treemacs-mode.el`、`treemacs-interface.el`、`treemacs-rendering.el`、`treemacs-customization.el`、`treemacs-follow-mode.el` 等源码；未运行 Treemacs、未做真实 command loop / GUI / IME 观察。源码结论不是运行观察，也不是 semacs 通过证据。

## 上游行为（静态）

### 窗口与入口

- 默认使用专用 side window，位于左侧，宽度 35；`treemacs-display-in-side-window` 为 t。side window 不会像普通 split 那样被 `delete-other-windows` 等操作轻易关闭。
- `treemacs` 在可见 / 已存在 / 不存在之间切换：可见时关闭窗口，已存在时显示并选择，不存在时初始化；工作区为空时询问首个项目根。
- `treemacs-select-window` 选择树窗口；已在树中时的行为由 `treemacs-select-when-already-in-treemacs` 决定（默认 `move-back`，即回到最近使用的窗口）。
- `treemacs-find-file` 显示并聚焦当前文件；当前 buffer 没有文件时要求输入路径。`treemacs-quit` bury 树 buffer，`treemacs-kill-buffer` 真正 kill。

### 树与节点

- 树是普通文本 buffer，节点由文本与 text property / button 构成；展开时插入子节点文本，折叠时删除可见子节点文本，节点身份与状态由独立 DOM 维护。不是 GUI 控件树。
- 节点类型包括 root / project、目录、文件、tag；目录和文件分开排序。目录可递归展开，文件可展开 tag 列表（依赖 imenu 等扩展）。
- 默认 `TAB` 对 root / 目录 / 文件执行 toggle；`RET` 对目录 toggle，对文件按 `treemacs-visit-node-default` 访问，对 tag 跳转。文件节点的 TAB 在无 tags 时通常没有可见效果。
- 可选能力包括图标、git 状态注解、filewatch、follow-mode、peek-mode、workspaces、持久化、文件管理、远程 / Tramp。它们不是核心文本树的必要条件。
- 目录读取、git 状态与折叠目录扁平化都可能异步执行；展开 / 刷新需要处理过期结果与不可读目录。

### 默认键位（与首版相关的子集）

| 键 | 参照命令 / 行为 |
|---|---|
| `TAB` / `C-i` | `treemacs-TAB-action`：展开 / 折叠当前节点 |
| `RET` | `treemacs-RET-action`：目录 toggle，文件访问 |
| `n` / `p` | 下一个 / 上一个可见树行 |
| `M-n` / `M-p` | 下一个 / 上一个同层节点 |
| `u` | 到父节点 |
| `q` / `Q` | bury / kill 树 buffer |
| `r` / `g` | 刷新 |
| `t h` | 切换 dotfiles |
| `t i` | 切换隐藏 gitignored 文件 |
| `w` / `<` / `>` | 设置 / 减小 / 增大宽度 |
| `o` 前缀 | 在垂直 / 水平 split、其他窗口或外部程序访问节点 |
| `P` | peek-mode：临时预览 |
| `cf` / `cd` / `R` / `d` / `m` | 创建文件 / 目录、重命名、删除、移动 |
| `y` 前缀 | 复制绝对 / 相对 / 项目路径、文件名或文件 |
| `C-c C-p` / `C-c C-w` | 项目 / workspace 操作前缀 |
| `C-j` / `C-k` | 下一个 / 上一个项目 |

### 生命周期与状态

- 折叠状态、节点身份与项目 / workspace 状态属于树 buffer / 会话；刷新尽量保留已有折叠与位置。
- filewatch 与 follow-mode 是可选模式；follow-mode 随当前 buffer 定位并展开树节点，不要求所有用户开启。
- git 状态注解依赖异步 Git 读数；不可读目录、仓库变化、迟到结果都有失败或回退路径。

## semacs 方向（side window 已确认，其余待细化）

- 形态：独立文本 buffer，放在专用左侧 side window；side window 独立于普通 split。按 treemacs 语义建议固定宽度、`C-x 1` 不关闭、`q` bury、显式 toggle，具体交互待确认。复用现有 section 树、可见行映射、mode 局部键位、focus / 临时 window 语义，不另建 GUI 树控件或平行 DiffWidget 式组件。
- 根：先支持当前项目根的单根树；根来源沿用 project 根识别。首版不做 workspaces、多根、远程。
- 节点：root / 目录 / 文件；目录懒加载，目录在前、文件在后；折叠状态归 buffer，point / scroll 归 view；刷新保留可匹配的折叠与位置。
- 首版键位建议：`TAB` toggle、`RET` 目录 toggle / 文件访问、`n` / `p` 可见行、`u` 父节点、`M-n` / `M-p` 同层、`q` bury、`g` 刷新、`t h` dotfiles、`w` / `<` / `>` 宽度；`treemacs-find-file` 式当前文件定位。`M-n` / `M-p` 当前不是正文默认绑定，但可能与未来 paragraph 绑定或用户配置重叠，需在实施前确认默认值与覆盖规则。
- 首版不做：文件增删改 / 移动、workspaces、图标、tags / imenu、filewatch、peek、完整 git 注解、持久化、远程 / Tramp。它们是可以后续按实际需要添加的能力，不是永久非目标。
- 失败与边界：不可读目录、符号链接、项目根失效、刷新时节点消失、异步目录读取迟到、树 buffer 被另存或 kill；沿用现有目标 / 版本隔离与只读生成 buffer 规则。
- 性能：不在首版承诺新增预算；目录读取必须有界、可取消，避免递归扫描整棵树阻塞前台。需要测量时另行确认入口与门限。

## 待确认问题

- side window 的宽度、默认位置、`C-x 1` / `C-x 0` / `C-x o` 行为、toggle 命令与焦点返回；是否完全照搬 treemacs 的 no-delete-other-windows 语义。
- 默认键位是否严格照搬上表，还是保留 `C-n` / `C-p` 作为普通文本移动，只新增 `n` / `p` / `M-n` / `M-p`？
- 根识别只认 `.git`，还是同时接受手动目录 / marker 文件？
- dotfiles 与 gitignore 的默认显示策略；首版是否需要隐藏 gitignored 文件。
- 当前文件高亮 / follow 是否进入首版，还是只保留显式 `treemacs-find-file` 定位？
- 文件访问目标：同窗口、最近使用窗口，还是固定的“其他 split”？首版是否需要 `o` 前缀。
