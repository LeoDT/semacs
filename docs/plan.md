# 实施计划

本文是当前工作和跨会话待办的唯一入口；维护与归档规则见 [文档说明](README.md#维护规则)。范围与当前方向见 [需求](requirements.md#当前方向v1-后)，行为见 [保真度契约](emacs-fidelity.md)。里程碑按可验收行为划分，不按工期估算。

## 当前状态：基础功能收尾，方向调整为代码查看

R0–F5、日常编辑增强及 F5 后插入项均已完成，交付摘要见 [R0–F1 归档](archive/completed-through-f1.md)、[F2 归档](archive/f2-status.md)、[F3 归档](archive/f3-window-commit.md)、[F4 归档](archive/f4-status-hunks.md)、[F5 归档](archive/f5-history-commit.md) 与 [F5 后插入项归档](archive/post-f5-insertions.md)。固定源码 / 环境、结果和未测边界见[冻结记录索引](records/README.md)。

没有已知阻塞性遗留；已接受的性能观察、字体 / 中文输入差异和未测边界保持原结论，不因功能收尾或方向调整视为预算通过。

用户已确认方向调整：从继续复刻 Emacs / Magit 基础功能，转为“使用 Emacs 操作逻辑的代码查看工具”。基础编辑保留；后续编辑能力以自动 / 智能缩进为主；主要精力放在 diff、文件夹 tree viewer，并加入基础 project 功能。已交付的 Git 写入不回退，但不自动扩展为完整 Magit / Git 客户端。长期边界见 [D29](decisions.md#d29-代码查看优先)。

## tree viewer 之前的小增强（已交付）

用户此前确认的三项小增强均已完成，行为分别见[F2 motion 差异](emacs-fidelity.md#已登记的-f2-motion-差异)、[独立 diff 查看](emacs-fidelity.md#独立-diff-查看)、[鼠标滚轮](emacs-fidelity.md#鼠标滚轮)与[候选式读入](emacs-fidelity.md#候选式读入阶段-b)；上游静态参照的版本保留在对应契约条目内。

1. point 离开视口时按 Emacs redisplay 居中，`M->` 按 `end-of-buffer` 的 `(recenter -3)` 定位，Magit `n` / `p` 跳到放不下的 hunk 时把 hunk heading 置顶。
2. 鼠标滚轮按 `mwheel-scroll` / `scroll-up` / `scroll-down` 调整窗口与 point，含 Shift 水平滚动、临时截断与归零恢复；GPUI 量、整行累计和收件时间加速适配已确认。
3. find-file 候选按 Vertico 的 history → length → alpha 排序（fuzzy 只做过滤），注解照 Marginalia `left` 格式，新增持久化 file-name history；mtime 不参与排序，不补 `./` / `../`。

`kill-current-buffer`（`C-x C-k`，含 minibuffer 内退出会话）与 Magit `S-TAB` 全局 section 可见性也已交付。这些改动不改变已交付 F1–F5 的契约。

## 下一步方向（方向已确认，首版待细化）

用户已确认：tree viewer 使用文本 UI 和 Emacs 操作逻辑，参考 treemacs；project 先做基础能力；diff 以查看为主，补 range / revision；indent 只记录、不排期。以下具体首版建议仍需确认，不是已批准实施阶段；交互细节与验收边界确认后才进入实施。

### 1. 文件夹 tree viewer（下一步主要方向）

- 形态（已确认）：独立文本 buffer + 专用左侧 side window；side window 独立于普通 split，不新建 GUI 树控件。建议固定宽度、`C-x 1` 不关闭、`q` bury、显式 toggle；具体交互待细化。复用现有 section 树、可视行、多 view、mode 局部键位与临时 window 语义；直接复用 section 还是建立专用 filetree 模型，在细化时确认。
- 建议首版：单项目根、目录 / 文件树、懒加载、展开折叠、键盘导航、打开文件、刷新、dotfiles 开关、当前文件定位。
- 建议首版不做：文件增删改 / 移动、workspaces、图标、tags / imenu、filewatch、peek、完整 git 注解、持久化、远程 / Tramp。
- side window 细节（宽度、位置、`C-x 1` / `C-x 0` / `C-x o`、toggle 命令与焦点返回）按 treemacs 参照在实施前确定，不现在锁定。
- 待定：键位是否照搬 treemacs（`n` / `p`、`M-n` / `M-p` 在 tree mode 内生效；需确认默认值与用户配置边界）、根来源、dotfiles / gitignore 策略、follow / 当前文件高亮是否进入首版。
- 依赖：项目根识别可作为 tree viewer 的前置；项目切换与项目内文件查找可在 tree 之后。tree viewer 之前的小增强已交付。
- 上游参照见 [treemacs 参照](work/treemacs-reference.md)；其中建议不是契约。

### 2. 基础 project

- 首版范围：项目根识别（先 `.git`，手动目录作为兜底）、项目切换、项目内文件查找。
- 项目内内容搜索是已确认的增强方向（搜索优先）；是否并入 project 首版待定，不默认扩大。
- 不预设完整 `project.el` 兼容、多根 / workspace 或远程 / TRAMP 支持。

### 3. diff range / revision

- 在现有 status / diff / log / revision 查看器上增加任意 revision 与 range 的查看；命令名、输入格式与从 log 选择 range 的交互待定。
- 比较身份固定到解析后的 OID / range，读取失败保留旧投影；不自动保存编辑 buffer。
- 不扩展 Git 写工作流；已交付的 stage / commit / amend / reword 维持现有契约。

### 4. 自动 / 智能缩进（仅记录）

- 需求保留；触发时机、语言范围、tree-sitter 规则与是否包含格式化均未定，不排期。
- 复用 [D24](decisions.md#d24-缩进边界) 的缩进 / 编辑分离和唯一 Edit 入口；不引入 major-mode 插件框架。

## 已确认的浏览体验增强（未排期）

用户已确认以下方向都想要，搜索相关优先；代码折叠与 jump list / 返回明确不纳入当前方向。以下仍不是已批准的实施顺序。

### 搜索（优先）

- **isearch / diff 内搜索**：当前 buffer 增量搜索；diff 隐藏 section 的展开 / 收起策略需单独定义。
- **项目内搜索 / occur**：ripgrep 或匹配行列表，结果可跳转；依赖 project 根、有界进程与结果 buffer。
- **搜索范围与结果呈现**：项目 / buffer / diff 的范围选择、匹配高亮、结果列表与重复搜索；结果跳转沿用现有文件 / 位置打开方式，不新增 jump list。

### 结构导航与阅读辅助

- **imenu / tree-sitter outline**：按函数 / 类 / Markdown 标题列出并跳转；复用现有语言查询，不引入 LSP。
- **面包屑 / 当前符号**：在 mode line 或 header 显示路径与 tree-sitter 外层结构；不依赖 LSP。
- **更多语言 / 符号导航**：按实际代码库补 grammar 与 outline 查询，不预建语言平台。

### tree 集成

- **tree follow / 当前文件定位**：切换 buffer 时高亮并展开当前文件；参考 treemacs follow-mode。
- **tree git 注解**：显示文件状态；复用现有 Git 读取与刷新边界。
- **peek / 临时预览**：从 tree 短暂查看文件而不切换焦点；复用 F3 临时 window 语义。

### 持久化

- **最近项目 / 文件、树状态持久化**：按实际使用决定，不提前建设。

### 明确不纳入当前方向

- **代码折叠**：按 tree-sitter 结构折叠普通文件。
- **jump list / 返回**：在文件、outline、搜索结果之间跳转后回到上一位置。

这两项用户已明确不纳入；如以后重新提出，按新方向确认，不因本列表自动恢复。

## 已知问题与未排期事项

### 已接受的暂缓项与观察

这些事项尚未修复，不自动重开或追加宿主矩阵；若实际影响使用，再确认最小定向范围。

- **首次命令准备 draw 峰值**：small 三轮首次命令准备 draw 35.408–37.827 ms >16 ms，其中 prepaint 34.355–36.640 ms；后续 diff 动作没有相应超限。[静态调查](records/f1-command-prepaint-inspection.md)指向首次粗体候选的字体匹配冷路径，精确根因 / 占比未证实。用户接受为非阻塞观察；不与 E3 字体切换问题合并，不记作总体预算通过。以后测量遵循[冷 / 热路径规则](performance.md#适用原则)：同进程相关字体使用可能建立缓存，纯文本浏览不保证如此，重启不能沿用进程内缓存；不能静默预热或跳过首次峰值。[宿主证据](records/f1-diff-diagnostic-host-01.md)保留非验收、预算未通过的结论。
- **字体切换 draw 峰值**：单 mixed 完整诊断的字体阶段 draw 50.328 ms >16 ms、首场景 50.664 ms >50 ms；完整 ready 229.014 ms，常规滚动 / 输入正常。用户接受当前速度并暂缓定位；原预算及超限结论不变，不视为全部性能通过。见 [完整诊断](records/e3-display-diagnostic-host-01.md)。
- **中文输入轻微视觉跳动**：E3 宿主复核中同行英文约向下跳 1–2 px，用户接受暂不修复；尚未量化行高 / 基线变化或确认根因。见 [人工记录](records/e3-host-manual-02.md)。
- **自定义缺失字体族 fallback**：GPUI 固定 fallback 丢失请求字重 / 斜体，通用名 `monospace` 未作为系统别名解析。随包 Lilex 只解决默认字体路径，显式指定上述名称仍可能回退普通字体；跨平台别名或依赖修复另行确认。见 [解析诊断](records/e3-font-resolution-host-01.md)。
- **多 view 聚合性能观察**：六类 headless 双 view 全部 ready 曾约 350 ms；聚合没有独立数值门限，不能套用旧单 view / 选中 view 的 300 ms 预算判作超限。用户已接受该非阻塞观察；不外推两个原生代表样本为六类全覆盖。只有出现实际响应问题或确认新预算时再分析；历史边界见 [阶段 C 归档](archive/completed-through-f1.md#阶段-cbuffer--view-与-split)。

### 未排期需求

- **密码 / passphrase 交互**：保留后续按 Magit 细化提示识别、保密读入（遮蔽与历史隔离）、取消及进程归属的需求。只有继续扩展 Git 提交流程时才提升优先级；需先落实输入基础设施并确认范围，不使用普通文本读入冒充支持。
- **任务持久化与恢复**：认可核心 Job、跨 reload 存活和分级恢复的方向，尚未批准实现或存储方案；具体待决项见[调研](work/scripting-extension-research.md#9-待确认事项)。先由实际任务推动，不能把自动重跑视为安全恢复。
- **鼠标多击 / Shift / 右键交互**：基础选区已交付；双击选词、三击选行（含行尾换行）、Shift 调整 region、右键 `mouse-save-then-kill` / `mouse-drag-copy-region`、拖动移动文本尚未实现。该交互不属于当前查看优先方向，暂缓；Emacs 参照边界见 [M3 记录](../tests/emacs/RESULTS.md#m3鼠标选区与文件读入预填参照)，实施前再确认范围。
- **自动保存与备份**：保留为数据安全增强需求；排期前先确认触发、写入位置与恢复行为。
- **更完整的 Emacs 编辑命令**：`M-y`、`C-u`、更完整的 kill / mark 交互等不在当前方向；实际使用受阻时再单独确认最小范围，不因既有 v1 列表自动补齐。

## 完成标准

各阶段按已确认范围交付，执行 [本地检查](acceptance.md#本地检查与-headless-入口) 及按影响选择的回归；真实环境验证另行取得同意，未测不记通过。收尾时按 [维护检查点](README.md#三个维护检查点) 更新当前工作、保留重要遗留事项并归档必要证据，不继续在本文累积已完成阶段的过程和测试清单。
