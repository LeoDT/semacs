# F：Magit diff 行为参考

本文是 F1–F5 实现前后使用的外部参照调查，**不是已批准的 semacs 行为契约或实施规格**。已采用部分以 [Emacs 保真度](../emacs-fidelity.md) 为准；未采用部分仍须另行确认，工作状态以 [实施计划](../plan.md) 为准。

## 参照身份与证据边界

- 用户提供源码：`/home/leodt/ghq/github.com/magit/magit`。
- 固定版本：`v4.7.1-74-g942d61d5`，commit `942d61d5de37c89a3a7c6cf693696b900b1d97b0`，调查时工作区干净。这是开发快照，不称作稳定发布版，也不假定等于用户日常使用版本。
- 依据同一 commit 的 `docs/magit.org`、`lisp/magit-section.el`、`lisp/magit-diff.el`、`lisp/magit-mode.el`、`lisp/magit-extras.el`。下文行号均指该 commit；上游固定链接见文末。
- 本轮仅静态阅读手册与源码；未运行 Magit、真实 command loop、GUI、Git 写操作或用户配置。不把源码结论当运行观察，不把外部参照当 semacs 已实现 / 验收通过。
- 之前在线查阅的手册及 v4.4.2 材料不作为本记录的固定基准；这里统一采用用户提供版本。

## 1. 目标界面与数据来源

目标是 Magit 的 unified diff 交互，不是 Emacs 自带 `diff-mode`，也不是左右并排比较器。

独立 `magit-diff-mode` 的基本结构：

```text
diff buffer（比较对象、参数与文件过滤条件）
├─ diffstat
│  └─ 文件统计条目
├─ 文件 section：状态、路径（重命名时含旧路径）
│  ├─ hunk heading：@@ 旧范围 新范围 @@ 与可选说明
│  │  └─ 上下文 / 删除 / 新增行
│  └─ 其他 hunk
└─ 其他文件
```

`diffstat` 与 diff 文件 section 是不同节点，不能仅凭文件路径将它们视为同一个 section。模式默认参数含 `--stat`、`--no-ext-diff`；Git patch 经解析后显示紧凑的文件 heading，而不是原样堆放所有 extended headers。

常用数据来源：`d u` 为 index → worktree，`d s` 为 HEAD → index，`d w` 为 HEAD → worktree，`d r` 指定 revision / range，`d p` 比较磁盘路径。`d d` 根据当前位置选择比较对象；展示 commit / stash 使用相关专用 buffer。status buffer 内嵌 staged / unstaged diffs，与独立 diff buffer 共享 section 机制但不完全同义。

证据：`magit-diff.el:1103–1117`、`:2372–2468`、`:2553–2560`、`:2670–2713`、`:2715–2882`。

## 2. 折叠行为

### 普通切换与递归循环不同

- `TAB`（`magit-section-toggle`）：切换当前 section 的 **body**，heading 留下。可在 section 正文中调用，不要求 point 位于 heading。
- 隐藏文件不改写各子 hunk 自己的 `hidden` 状态；再次普通展开会恢复子 hunk 的原状态。
- 收起时，若当前 point 严格落在即将隐藏的正文范围内，将它移至 section 起点。根 section 不允许隐藏。
- `C-c TAB` / `C-<tab>`：循环当前 section 与子树的可见层级；在普通文件 → hunk 树上可理解为“文件收起 → 仅 hunk headings → hunk 正文展开 → 文件收起”。更深的树、混合状态由实际子节点状态决定，不是所有节点共用一个三态数字。`C-<tab>` 还可能让位于 Emacs 的 tab-bar 切换。
- `M-<tab>`：仅循环当前 buffer 的 diff 相关 section；status 中针对 staged / unstaged 下的文件，独立 diff 中针对根下的文件。
- `S-<tab>`：循环整个 buffer 的 section，可影响 diffstat 等非 diff 正文 section；不等同 `M-<tab>`。
- `1`–`4`：设置当前位置周围的可见层级；`M-1`–`M-4`：对全 buffer 设置层级。不是固定对应“1 文件 / 2 hunk / 3 正文”，层级取决于所属树。
- 支持图形 fringe / margin 折叠指示、终端 heading 末尾省略号；heading 双击或指示区点击可切换。不是任意正文单击都切换。

证据：`magit-section.el:445–485`、`:945–1122`；`magit-diff.el:2256–2285`。

### 初始状态与刷新

- 独立 diff 中普通文件与 hunk 通常展开；**deleted 文件默认收起**。status buffer 中的文件默认收起，不能用 status 的观察替代独立 diff 初始行为。
- 可见性 hook / 配置可覆盖默认；刷新洗出耗时超过 `magit-diff-expansion-threshold`（默认 60 秒）后，后续文件可被收起。这是上游同步处理的保护，不是 semacs 应复制的性能目标。
- 普通刷新尽量保留已有 section 的可见性；默认也缓存暂时消失、之后重现的 section 状态。
- section 身份由类型、value 与父链组成。文件节点通常以路径为 value；hunk value 含说明及范围，内容变化造成身份变化时，不能保证仍匹配旧 hunk。
- 折叠树与缓存属于 **buffer**，同一 buffer 的多窗口共享折叠状态；不是每 window 独立一份。

证据：`magit-diff.el:136–153`、`:2857–2882`、`:2985–3021`；`magit-section.el:167–210`、`:577–617`、`:1454–1480`、`:2007–2030`。

## 3. 导航与滚动

| 默认键位 | 参照行为 |
|---|---|
| `n` | 下一个可见 section 起点；展开的文件可先进入其 hunk，收起的子树被跳过 |
| `p` | 一般先回当前 section 起点；已经在起点时到前一个可见 section，另有 buffer / section 末尾特例 |
| `M-n` / `M-p` | 优先下一个 / 上一个兄弟 section，跳过当前子树 |
| `^` | 父 section 起点 |
| `SPC` / `DEL`（或 `S-SPC`） | 独立 diff buffer 中向下 / 向上翻屏，不是 section 导航 |
| `j` | diffstat 的文件项与对应 diff 之间跳转；没有 diffstat 时报告错误 |
| `q` | bury 当前 Magit buffer，结合窗口恢复策略；不是普通文本编辑中的删除或一定 kill buffer |

普通 `C-n` / `C-p` 等文本移动仍可用。专用 section 导航会触发 movement hook；进入 hunk 时有确保 hunk 起点可见的窗口调整，不能仅把它实现成裸 point 跳转。

默认 `magit-section-mode` 只读、禁用文本 undo，并设置 `truncate-lines = t`：**长行默认不软换行**。semacs 当前全文软换行不能不经确认直接当作同等体验。

上游内部不一致需留待核对：兄弟导航的 docstring / 手册说“无兄弟时到父节点”，但此快照实际函数回退调用普通 forward / backward；例如最后一个 hunk 的 forward fallback 可能走向下一文件，而非停在父文件。暂不将说明或静态推演直接定为 semacs 契约，后续用隔离 case 观察并确认采用哪种行为。

证据：`magit-section.el:457–502`、`:803–893`；`magit-diff.el:2349–2383`、`:3437–3443`；`magit-mode.el:352–400`、`magit-mode-bury-buffer`。

## 4. 当前 section、选择与视觉反馈

Magit 不只是增删行着色，还区分：文件 heading、hunk heading、当前 section、选中的 sections、hunk 内选中行，以及可选的词级差异。

- 当前 section 高亮与 active region 是两件事；默认会突出当前位置所属 section，hunk body 也有对应高亮能力。
- 多 section 选择由 Emacs region 派生。典型有效选择从某个 heading 开始，到其兄弟 heading 结束；命令还可限制可操作的 section 类型。
- region 末端恰好位于下一个 heading 的行首时，该 section **仍被包含**，不同于普通文本 region 的半开范围。
- 无效 section 选择不等于“对任意跨越的所有节点操作”；许多命令会只作用于 point 所在 section。单 section 选择另有活动 mark 等特殊条件，不能泛化为任意 heading 内文本选区都有效。
- hunk 内部分行选择要求 region 两端都在**同一个 hunk 正文**中；其行范围也包括末端所在行。不能拿跨 hunk 的普通文本选择直接冒充部分 hunk patch。
- hunk 内选择会强调选中部分，并弱化外部增删行。stage / unstage / apply 等命令消费这些结构化范围，不只是复制几行文本。
- `D t` 切换词级差异；`magit-diff-refine-hunk` 默认 `nil`，支持全部或到达 hunk 后细化。`D T` 控制 hunk 语法着色，`magit-diff-fontify-hunk` 默认也是 `nil`。词级细化和语法着色不是同一能力，不是“Magit 默认全开”。

证据：`docs/magit.org:1672–1745`；`magit-section.el:145–158`、`:2213–2312`；`magit-diff.el:155–200`、`:202–245`、`:1798–1842`、`:3597–3635`、`:3943–4000`。

## 5. 参数、上下文与刷新

- `d` 选择“看哪个 diff”；`D` 修改当前 buffer 的 diff 参数，不必重新选择比较对象。
- `+` / `-` 增减上下文行数，最少为 0；`0` 恢复默认上下文（Git `diff.context`，未配置时为 3）。这些命令修改参数并刷新 Git diff，**不是展开已在 buffer 中的隐藏行**，可能导致 hunk 合并、拆分或身份变化。
- `D` 可修改文件过滤、忽略空白、算法、重命名检测等参数；当前 buffer 参数、同模式默认参数和持久保存默认值有区别。
- `g` 刷新当前 Magit buffer，并按设置刷新关联 status buffer；不是保证“只刷新当前屏幕”。
- 刷新前保存各 window 的 section 身份、section 内相对行列及 window start，再在新树中尽力恢复。section 消失或身份改变时走 successor / fallback，不承诺字节级原位置必然保留。
- 同一 buffer 多窗口共享 diff 与折叠树，但分别恢复各自 point / 滚动位置。

证据：`magit-diff.el:1032–1069`、`:1078–1150`、`:1743–1783`；`magit-mode.el:1046–1080`、`:1097–1211`。

## 6. 从 diff 访问文件版本

- `RET` 访问当前 diff 行对应的**适当版本**并定位；删除行默认选择旧 / 左侧，新增或上下文行默认选择新 / 右侧。
- staged 比较为 HEAD 与 index；unstaged 比较为 index（无该文件 staged 变更时可用 HEAD）与 worktree。因此 staged 中的 `RET` 通常不是访问可编辑的工作区文件。
- `C-<return>` 明确访问 worktree 文件，即使当前 diff 是历史 revision 或 staged；位置映射会考虑中间变更，不是简单套用 diff 的新行号。
- 支持另一 window / frame 变体；有前缀参数时也可选择另一 window。
- 重命名需要分别保存旧 / 新路径。删除文件、缺失 worktree、binary、merge 等不能凭一条普通新行号跳转规则兜底；本轮未逐分支运行核对。

证据：`docs/magit.org:4267–4371`；`magit-diff.el:602–640`、`:1845–1957`、`:1999–2095`、`:2470–2492`。

## 7. 搜索、复制与特殊 diff 的边界

- 隐藏通过 overlay 保留正文，而不是从 buffer 删除文本。
- isearch 有专门临时打开隐藏 section 及祖先的逻辑，并在搜索清理时根据最终 point 决定哪些重新收起；不能外推为任意搜索命令都如此。
- `magit-copy-section-value` 有复制 section value、region，以及带前缀复制 hunk 某一侧文本并移除 diff 标记列的不同分支。普通复制跨隐藏内容是否包含正文、取消搜索后的精确状态，仍需隔离运行验证；不以“用了 invisible overlay”代替观察结论。
- Magit 还识别 binary、纯 rename / chmod（可作为特殊 hunk）、新建 / 删除、submodule、combined / conflict diff。第一版若暂不支持某种结构，应明确显示摘要或拒绝，不伪造成普通可逐行操作的 hunk。

证据：`magit-section.el:960–976`、`:2124–2151`；`magit-mode.el:570–577`；`magit-extras.el:651–684`；`magit-diff.el:2715–3021`。

## 8. 对 semacs 的含义（待确认建议）

1. 最小查看切片应围绕结构化 file / hunk section、真实可见行映射、局部键位、当前 section 反馈、折叠与导航展开；不先造完整通用 overlay / major-mode / transient 框架。
2. 长行截断 / 水平查看是当前显示结构之外的一项缺口；采用 Magit 默认还是保留软换行，需要明确批准。
3. 若希望第一版就查看实际仓库 staged / unstaged diff，需要同时加入最小 Git 执行、身份 / 版本隔离和刷新；“先读任意 `.diff` 文件”并不自动等于用户要的 Magit diff。
4. 词级差异、diffstat、上下文调整、旧侧 / index blob 访问、搜索自动展开等，应按使用频率决定首版范围，不能因 Magit 有就全部视为前置阻塞。
5. stage / unstage / discard / apply 是后续 Git 写操作切片；未来设计必须消费结构化 section / hunk 选区并考虑仓库变化、确认与失败恢复。查看器阶段不先实现所有写操作，也不预先承诺支持它们。
6. Rust 拥有进程、diff 模型与显示原语；若以 JS 组合工作流，现有仅支持 region 文本变换的脚本 API 还不足，应按实际切片增量补充，遵循 D26，不另建平行状态系统。

尚需用户确认：日常 Magit 版本 / 定制（若要对齐个人行为）、独立 diff 与 status 内嵌的首版范围、默认换行策略、哪些导航 / 选择 / 跳转是首版必需。以上不代表开始 F 实施。

### 后续可复现观察建议（尚未执行）

使用独立 `emacs -Q`、固定依赖、`.pi/tmp/` 下可丢弃 Git fixture；不得加载日常配置或操作真实仓库。优先固定：

- 两文件、多 hunk、混合折叠状态，核对 TAB / cycle / 可见导航及无兄弟 fallback。
- 刷新前后 hunk 行号变化、节点消失重现、双 window 位置恢复。
- heading 多选、行首末端、hunk 内行选择与跨 hunk 无效选择。
- staged / unstaged、删除行、rename、历史版本下的 RET / C-RET。
- 搜索命中隐藏正文、退出 / 取消后折叠状态，普通复制跨隐藏区域。

这些可以先用隔离 batch / ERT 固定语义，但不证明真实 command loop、鼠标 / GUI 或长行横向查看体验。若需真实桌面测试，另行征求用户同意。

## 固定上游来源

- [手册](https://github.com/magit/magit/blob/942d61d5de37c89a3a7c6cf693696b900b1d97b0/docs/magit.org)
- [section 实现](https://github.com/magit/magit/blob/942d61d5de37c89a3a7c6cf693696b900b1d97b0/lisp/magit-section.el)
- [diff 实现](https://github.com/magit/magit/blob/942d61d5de37c89a3a7c6cf693696b900b1d97b0/lisp/magit-diff.el)
- [mode / refresh 实现](https://github.com/magit/magit/blob/942d61d5de37c89a3a7c6cf693696b900b1d97b0/lisp/magit-mode.el)
- [复制等扩展命令](https://github.com/magit/magit/blob/942d61d5de37c89a3a7c6cf693696b900b1d97b0/lisp/magit-extras.el)
