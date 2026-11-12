# tree viewer 前的小增强：参照与行为记录

> 本文记录用户已确认要在 tree viewer 之前处理的小增强及静态参照，**不是已批准的行为契约或实施规格**。具体键位、默认值和验收边界在实施前确定；当前计划见 [实施计划](../plan.md)。

## 来源与证据边界

- Emacs 31.1：`/usr/share/emacs/31.1/lisp/mwheel.el.gz`、`simple.el.gz`；变量默认值经 `emacs -Q --batch` 打印确认。
- Magit：`~/.emacs.d/elpa/magit-20260822.1158`（commit `137f137dccb37097ae9caa31018cafa071ff4ec1`）与 `magit-section-20260731.2248`（commit `67f203853e74e926e2c99f60ed508840714f7ced`）。
- Marginalia：`~/.emacs.d/elpa/marginalia-20260821.1231`（commit `7ec0b70afb43a756ecd45a958c6ebe797717fc91`）；用户同时使用 Vertico。
- 用户 Emacs 配置：`C-x C-k` 绑定 `kill-current-buffer`。
- 仅静态阅读源码与 docstring；未做真实 GUI 鼠标滚轮 / 重绘观察。涉及屏幕实际表现的结论保留为待验证。

## 1. point 离开视口时的滚动

**现象 / 目标**：浏览代码时 point 到页底再向下移动一行，用户观察到 Emacs 会大致滚动半页，而不是只向下滚一行。实际行为按 Emacs 默认 redisplay 处理；diff 中用 `n` / `p` 导航 section 时，按 Magit 的 section 移动逻辑保证目标内容可见，不让 hunk header 停在页底而看不到正文。

**静态依据**：

- Emacs 31.1 默认 `scroll-conservatively` = 0、`scroll-step` = 0、`scroll-margin` = 0。`scroll-conservatively` 的 docstring：值为 0 表示 point 离开屏幕时总是 recenter；`scroll-step` 为 0 时同样总是居中。
- Magit `magit-section-goto` 只移动 point 并运行 `magit-section-movement-hook`；`magit-hunk-set-window-start` 在该 hook 中保证 hunk section 起点可见：若 hunk 末尾不在窗口内，则把 window-start 设到 hunk 起点。普通 section 没有该 hook，依赖 Emacs redisplay 的默认滚动。
- semacs 当前 `BufferView` 的 reveal 只做最小滚动，把 point 刚好带入视口；这是用户观察到“只滚一行”的直接差异。

**实施原则**：普通 motion 按 Emacs redisplay 默认 recenter，不另造“固定滚动半页”常量；hunk 导航按 Magit `magit-hunk-set-window-start` 的 window-start 规则，其他 section 不额外发明可见性约束。实施前只需核对与现有 `C-l`、翻页、`goal column`、软换行和 sections 投影的交互。

## 2. Magit 的 Shift+Tab 全局 section 可见性

**现象 / 目标**：diff / Magit 中 `Shift+Tab` 应能全局展开 / 收起顶级 section，例如 diff 中的文件、status 中的 staged / unstaged 组。实际行为按 Magit 上游 cycle 语义。

**静态依据**：

- Magit `magit-section-mode-map` 把 `<backtab>` 绑定到 `magit-section-cycle-global`。
- `magit-section-cycle-global` 对 root 的子 section 做三态循环，不是简单二元 toggle：
  1. 若有子 section 隐藏、且部分子 section 有 children：显示所有 headings、隐藏正文；
  2. 否则若有子 section 的正文隐藏：显示所有 children 正文；
  3. 否则隐藏所有顶级子 section。
- 该行为适用于所有基于 `magit-section-mode` 的 buffer，包括 diff、status、process、log、log-select、revision；顶级 section 类型随 mode 不同。

**实施原则**：按 Magit 上游的三态 cycle 实现，不另做简化的二元 toggle。普通 `TAB` 只切换当前 section；`S-TAB` 只做全局 cycle。折叠状态、point 与多 view 的恢复规则沿用现有 section 模型。

## 3. 鼠标滚轮与 point

**现象 / 目标**：semacs 的鼠标滚动只改视图、不改 point。实际逻辑按 Emacs `mwheel-scroll` / `scroll-up` / `scroll-down` 的语义，不按“滚轮等于 point 移动一行”的观察简化。

**静态依据**：

- Emacs `mwheel-scroll` 不直接绑定 `next-line`；它按 `mouse-wheel-scroll-amount` 调用 `scroll-up` / `scroll-down`。Emacs 31.1 默认 amount 为 1 行，`mouse-wheel-progressive-speed` 为 t。
- `scroll-up` / `scroll-down` 都带 `scroll-command` 属性；默认 `scroll-preserve-screen-position` 为 nil，表示 point 不保持屏幕位置，但在滚动余量 / 窗口边界处会被调整。
- 因此“滚轮 = 光标移动一行”是不准确的简化：实际是滚动窗口，point 在边界条件下调整。精确屏幕上表现仍需要真实 GUI 观察。
- semacs 当前滚轮只调用 `scroll_by`，point 不变，光标可能离开可视区。

**实施原则**：按 Emacs `mwheel-scroll` / `scroll-up` / `scroll-down` 的 scroll-command 语义实现，不改成 `next-line`。默认 amount 为 1 行、progressive speed 为 t 可作为起点；边界处 point 落点及水平滚动继续按 Emacs 行为核对。精确屏幕表现实施前在真实 Emacs GUI 与 semacs 桌面上做一次对照观察，不把观察现象直接写成新契约。

## 4. find-file 候选的排序与信息

**现象 / 目标**：用户观察到 find-file 候选有排序、并显示更多信息；实际候选顺序与注解按 Emacs completion 及用户现有 completion 配置核对，不把“按 mtime 排序”直接当成 Emacs 默认。

**静态依据**：

- 用户日常 Emacs 使用 Vertico + Marginalia；`marginalia-annotate-file` 对本地文件显示文件权限 / 类型、人类可读大小、修改时间，以及必要时 owner。远程路径只显示 remote 标记。
- Marginalia 只负责注解；Vertico 默认排序是 history / length / alpha，不是按 mtime。
- semacs 当前 `path_candidates` 使用 `fs::read_dir` 的直接顺序，注解只有 `file` / `directory`。

**实施原则 / 实施前核对**：

- 候选顺序遵循 Emacs completion 的实际排序语义，包括 `display-sort-function` 和用户配置可观察到的顺序；不自行固定 mtime 排序。mtime 先作为注解字段，是否参与排序按 Emacs / 用户配置核对。
- 注解参考用户 Emacs 的 Marginalia：权限 / 类型、人类可读大小、修改时间、必要时 owner；窄窗口下截断。
- 每个候选读取 metadata 的成本、缓存与有界更新需要在实施前测量；避免在候选刷新中阻塞前台。
- 影响范围先限定文件候选，不自动改动 buffer / 命令候选的排序。

## 5. kill-current-buffer

**需求**：需要 `kill-current-buffer` 命令，行为按 Emacs 定义。

**静态依据**：

- Emacs 31.1 定义 `kill-current-buffer`，默认没有全局键位；`C-x k` 仍是 `kill-buffer`。
- 用户配置把 `C-x C-k` 绑定到 `kill-current-buffer`。
- 行为：在普通窗口调用 `kill-buffer` 当前 buffer；在 minibuffer 中调用 `abort-recursive-edit` 退出；dirty 确认沿用 `kill-buffer`。

**实施原则**：semacs 默认键位采用用户配置的 `C-x C-k`；`C-x k` 保持 `kill-buffer`。minibuffer 中按 Emacs `abort-recursive-edit` 退出当前会话，与现有 `C-g` 会话取消路径合并，不另建语义。dirty buffer 确认、退出保存协调及最后 buffer 回退沿用现有 kill-buffer 规则。
