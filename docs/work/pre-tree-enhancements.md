# tree viewer 前的小增强：参照与行为记录

> 本文记录用户已确认要在 tree viewer 之前处理的小增强及静态参照，**不是已批准的行为契约或实施规格**。具体键位、默认值和验收边界在实施前确定；当前计划见 [实施计划](../plan.md)。

## 来源与证据边界

- Emacs 31.1：`/usr/share/emacs/31.1/lisp/mwheel.el.gz`、`simple.el.gz`；变量默认值经 `emacs -Q --batch` 打印确认。
- Magit：`~/.emacs.d/elpa/magit-20260822.1158`（commit `137f137dccb37097ae9caa31018cafa071ff4ec1`）与 `magit-section-20260731.2248`（commit `67f203853e74e926e2c99f60ed508840714f7ced`）。
- Marginalia：`~/.emacs.d/elpa/marginalia-20260821.1231`（commit `7ec0b70afb43a756ecd45a958c6ebe797717fc91`）；用户同时使用 Vertico。
- 用户 Emacs 配置：`C-x C-k` 绑定 `kill-current-buffer`。
- 仅静态阅读源码与 docstring；未做真实 GUI 鼠标滚轮 / 重绘观察。涉及屏幕实际表现的结论保留为待验证。
- 例外：#1 的 point 离开视口滚动另用 `tests/emacs/drive.py` 的 `m4-recenter-motion` 在真实 pty command loop 核对；仍不覆盖 GUI 像素布局、变高行或用户桌面。

## 1. point 离开视口时的滚动（已实施）

已实施：契约见[保真度](../emacs-fidelity.md#已登记的-f2-motion-差异)与[独立 diff 查看](../emacs-fidelity.md#独立-diff-查看)。

**现象 / 目标**：浏览代码时 point 到页底再向下移动一行，用户观察到 Emacs 会大致滚动半页，而不是只向下滚一行。实际行为按 Emacs 默认 redisplay 处理；diff 中用 `n` / `p` 导航 section 时，按 Magit 的 section 移动逻辑保证目标内容可见，不让 hunk header 停在页底而看不到正文。

**静态依据**：

- Emacs 31.1 默认 `scroll-conservatively` = 0、`scroll-step` = 0、`scroll-margin` = 0。`scroll-conservatively` 的 docstring：值为 0 表示 point 离开屏幕时总是 recenter；`scroll-step` 为 0 时同样总是居中。
- `end-of-buffer`（`simple.el`）在 `(> (point) (window-end nil t))` 时同步调用 `(recenter -3)`；`beginning-of-buffer` 没有命令级 recenter，只 `goto-char`。`recenter` 无参数即 `move-to-window-line` 的 `(displayed_window_lines)/2`。
- Magit `magit-section-goto` 只移动 point 并运行 `magit-section-movement-hook`；`magit-hunk-set-window-start` 在该 hook 中保证 hunk section 起点可见：若 hunk 末端不完全可见，就把 window-start 设到 hunk 起点。普通 section 没有该 hook，依赖 Emacs redisplay 的默认滚动。
- semacs 原 `BufferView` 的 reveal 只做最小滚动，把 point 刚好带入视口；这是用户观察到“只滚一行”的直接差异。

**真实 command loop 观察**（pty、24x80、21 行正文；用例 `m4-recenter-motion`，明细见 [RESULTS](../../tests/emacs/RESULTS.md#m4point-离开视口与-end-of-buffer-滚动参照)）：

- point 逐行下移直到正文最后一行都不滚动；再下移一行后 point 落在新窗口第 11 行（window-start 是 point 行前 10 行，21 行正文的中间），不是只滚一行。
- `M->` 由命令内的 `recenter -3` 同步定位：point 在第 19 行，下面留 2 行空白；不是 redisplay 的居中。
- `M-<` 经 redisplay 居中后受 buffer 首夹紧，window-start 回到首行。

**实施结果**：

- `BufferView` 的 reveal 改为「point 行不完全可见才滚动，且滚动后居中」，复用 `C-l` 居中同一套像素偏移；`M->` 通过独立的重心位置（距底部三行）请求，`beginning-of-buffer` 仍走 reveal。
- 这两类需要行定位的请求与 `C-l` 共用 `pending_scroll`，等兼容显示行就绪后应用；不再是 `pending_recenter` 单一枚举。
- section 导航在 `n` / `p` / `^` 跳到 hunk heading 时，用 hunk 末端那一行的可见性决定是否把 hunk heading 置顶；hunk 类型由渲染器在 `Section` 上标注，heading 字符串仍只作身份。Magit 隐藏正文的 overlay 止于 section end 之前，所以折叠只隐藏正文、不改变末端行是否可见；`j`、`RET`、鼠标跳转等其他入口不跑 `magit-section-movement-hook`，也不套用该规则。
- 已接受差异：定位按像素而非整行取整，不承诺 Emacs `move-to-window-line` 的半行差异与 `scroll-margin`；hunk 置顶要等兼容显示行就绪，索引完整却仍解析不出末端行时不置顶（不把请求永久挂起）。

## 2. Magit 的 Shift+Tab 全局 section 可见性（已实施）

已实施：契约见[保真度](../emacs-fidelity.md)。真实 Magit 的可见状态是两态循环（仍有正文隐藏时全部展开；全部正文可见时只折叠顶级 section），不是本稿原先记的“三态”；观察见 [冻结记录](../records/magit-section-cycle-global-observation.md)。

**静态依据**：Magit `magit-section-mode-map` 把 `<backtab>` 绑定到 `magit-section-cycle-global`；源码里的三个 cond 分支中，前两支（部分子 section 隐藏且有 children、深层正文隐藏）都展开全部正文，第三支才折叠顶级子 section，所以可见状态实际是两态循环。折叠使 point 落入隐藏正文时移回该 section heading。该命令适用于所有基于 `magit-section-mode` 的 buffer：diff、status、process、log、log-select、revision，顶级 section 类型随 mode 不同。

**实施结果**：普通 `TAB` 只切换当前 section；`S-TAB` 只做全局 cycle，折叠状态、point 与多 view 沿用现有 section 模型。

## 3. 鼠标滚轮与 point

**现象 / 目标**：semacs 的鼠标滚动只改视图、不改 point。实际逻辑按 Emacs `mwheel-scroll` / `scroll-up` / `scroll-down` 的语义，不按“滚轮等于 point 移动一行”的观察简化。

**静态依据**：

- Emacs `mwheel-scroll` 不直接绑定 `next-line`；它按 `mouse-wheel-scroll-amount` 调用 `scroll-up` / `scroll-down`。Emacs 31.1 默认 amount 为 1 行，`mouse-wheel-progressive-speed` 为 t。
- `scroll-up` / `scroll-down` 都带 `scroll-command` 属性；默认 `scroll-preserve-screen-position` 为 nil，表示 point 不保持屏幕位置，但在滚动余量 / 窗口边界处会被调整。
- 因此“滚轮 = 光标移动一行”是不准确的简化：实际是滚动窗口，point 在边界条件下调整。精确屏幕上表现仍需要真实 GUI 观察。
- semacs 当前滚轮只调用 `scroll_by`，point 不变，光标可能离开可视区。

**用户 GUI 观察（Emacs 31.1，真实桌面，窗口固定）**：单格 1 行、point 被推至窗口首 / 末行、首尾贴边及 region 取消均与上文一致；快速连滚能感到加速，但读不出每格行数；触摸板难以精确停在单行，但不是像素级滚动；`Shift+滚轮` 水平滚动一列。

**实施原则**：按 Emacs `mwheel-scroll` / `scroll-up` / `scroll-down` 的 scroll-command 语义实现，不改成 `next-line`。默认 amount 为 1 行、progressive speed 为 t 可作为起点；边界处 point 落点及水平滚动继续按 Emacs 行为核对。精确屏幕表现实施前在真实 Emacs GUI 与 semacs 桌面上做一次对照观察，不把观察现象直接写成新契约。

## 4. find-file 候选的排序与信息

**现象 / 目标**：用户观察到 find-file 候选有排序、并显示更多信息；实际候选顺序与注解按 Emacs completion 及用户现有 completion 配置核对，不把“按 mtime 排序”直接当成 Emacs 默认。

**静态依据**：

- 用户日常 Emacs 使用 Vertico + Marginalia；`marginalia-annotate-file` 对本地文件显示文件权限 / 类型、人类可读大小、修改时间，以及必要时 owner。远程路径只显示 remote 标记。
- Marginalia 只负责注解；Vertico 默认排序是 history / length / alpha，不是按 mtime。
- semacs 当前 `path_candidates` 使用 `fs::read_dir` 的直接顺序，注解只有 `file` / `directory`。

**已确认（实施前核对结论）**：

- 用户 Emacs 的 file completion 没有 `display-sort-function` / `annotation-function`，Vertico 用默认 `vertico-sort-history-length-alpha`，Marginalia 注解；用用户的 Vertico 与 `~/.emacs.d/history` 在 batch 复现了该顺序。mtime 不参与排序。
- 用户确认：新增持久化 file-name history 并参与排序；过滤保持现有 fuzzy，排序改为历史 → 长度 → 字母序；注解照抄 Marginalia left 对齐格式；不补 `./` / `../` 候选。

- 注解参考用户 Emacs 的 Marginalia：权限 / 类型、人类可读大小、修改时间、必要时 owner；窄窗口下截断。
- 每个候选读取 metadata 的成本、缓存与有界更新需要在实施前测量；避免在候选刷新中阻塞前台。
- 影响范围先限定文件候选，不自动改动 buffer / 命令候选的排序。

## 5. kill-current-buffer

已实施：`C-x C-k` 绑定 `kill-current-buffer`，minibuffer 内退出当前会话；契约见[保真度](../emacs-fidelity.md)。

**静态依据**：Emacs 31.1 的 `kill-current-buffer` 默认没有全局键位，`C-x k` 仍是 `kill-buffer`；用户配置把 `C-x C-k` 绑定到 `kill-current-buffer`，在 minibuffer 中调用 `abort-recursive-edit`。
