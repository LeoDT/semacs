# 实施计划

本文是当前工作和跨会话待办的唯一入口；维护与归档规则见 [文档说明](README.md#维护规则)。范围见 [需求](requirements.md)，行为见 [保真度契约](emacs-fidelity.md)。里程碑按可验收行为划分，不按工期估算。

## 当前状态：F5 提交历史与提交整理完成

R0–M3、A–E3、日常编辑增强及 F1 只读 Magit diff 的交付结论见 [R0–F1 归档](archive/completed-through-f1.md)，F2 status / 单文件 stage / unstage 见 [F2 归档](archive/f2-status.md)。F3 临时 window 与普通 commit 已完成实现、集中审查修复、本地验证及获准的单窗口宿主功能验证，见 [F3 归档](archive/f3-window-commit.md)；固定源码 / 环境、结果和未测边界见[冻结记录索引](records/README.md)。

F3 没有已知阻塞性遗留项；密码 / passphrase 读入已由用户明确排除于 F3，不因它继续维持阶段未完成。原性能预算、历史非阻塞观察及各阶段未测边界不变，不把功能收尾解释为全部性能预算通过。

## 已完成插入项：Magit 样式、M-x 历史与 scratch 退出

已交付语义样式 / view-local 高亮、M-x 历史排序及持久化、普通非文件 buffer 的退出保存豁免；本地验证与当时的 headless 证据见[记录](records/magit-style-history-scratch-local.md)。用户随后确认新样式已目视验证、没有问题；该确认不扩展为性能或其他宿主矩阵通过，也不改写冻结记录当时的未测结论。

## 已完成插入项：语法发布的局部强调重排

语法解析完成后的发布不再无条件重建整份 wrap 索引：显示文本与字体 / 宽度等输入不变、且新旧排版都保留语法快照时，后台比较按主题角色折叠后的有效强调 run（bold / italic）。完全一致只把新 `Typography` 记到索引 key；局部变化只重排首个到最后一个差异所在的逻辑行，并保留首个受影响行之前的 shaping 缓存。首次启用语法强调、空快照、单独的主题变化、`marked` / overlay / named / section 字体变化及 `visible_lines` 投影仍走原整份重建；与发布同帧的角色强调变化按实际差异处理。

已完成集中审查修复及获准的 P1 专项复核；本地默认 / native-profiler 全目标测试、两套 clippy、fmt 和 diff 检查通过。用户简单试用后反馈已明显改善，并同意提交；未提供具体操作覆盖或测量数据，不扩展为完整宿主矩阵或性能预算通过。助手未启动真实桌面；后台比较成本与宿主输入时延未测。

## 已完成阶段 F4：status 内嵌 diff 与单 hunk 操作

已交付批准范围，并完成一次集中审查修复、本地全目标检查及 status / 独立 diff headless 专项；没有已知阻塞性遗留。交付摘要见 [F4 归档](archive/f4-status-hunks.md)，固定身份、结果与未测边界见[本地记录](records/f4-status-hunks-local.md)。用户随后确认已试用、没有明显问题并同意收尾提交；未提供具体操作覆盖或测量数据，不扩展为完整宿主矩阵或原生性能通过。助手未启动真实桌面。

## 已完成阶段 F5：本地提交历史与提交整理

已交付批准范围（含历史 merge 的只读 combined 及 parent / result 版本访问），完成一次集中审查修复、默认 / profiler 全目标检查及独立 diff headless 回归，没有已知阻塞性遗留。摘要见 [F5 归档](archive/f5-history-commit.md)，固定源码、结果及未测边界见[本地记录](records/f5-history-commit-local.md)。用户随后确认已大致试用、没有明显问题，但未做完整测试，并同意先收尾提交；该反馈不扩展为完整宿主矩阵或原生性能通过，也不改写冻结记录当时的未测结论。助手未启动真实桌面。

## 已完成插入项：鼠标基础选区与文件路径预填

按已确认方案交付鼠标基础选区（单击、拖动、拖出上下边界自动滚动）与文件读入默认目录预填（`C-x C-f` / `C-x C-w` / 无路径 `C-x C-s` 共用解析，未编辑 RET、目录答案与默认文件名对齐已核实的 Emacs 语义）。

实现要点：`EditTarget` 新增按下 / 拖动 / 释放入口，`BufferView` 用单一拖动态与 250 ms 滚动任务管理自动滚动，窗口级监听只处理文本区外的指针；文件读入目录按“origin 仓库根 → 文件父目录 → 启动目录”解析，`$HOME` 仅按组件缩写，候选与确认共用同一解析，候选层的目录接受改为由确认层按 kind 决定（`find-file` 保留会话并提示，`write-file` 追加 buffer 文件名）。

契约见[鼠标选区](emacs-fidelity.md#鼠标选区)与[文件读入的默认目录与路径答案](emacs-fidelity.md#文件读入的默认目录与路径答案)；Emacs 31.1 的 pty 鼠标 / 读入观察见[参照记录](../tests/emacs/RESULTS.md#m3鼠标选区与文件读入预填参照)，不是 semacs 通过证据。已登记差异：单击不恢复点击前的 mark（上游 `pop-mark`）、缺少 dired、自动滚动按像素换算行。本地全目标测试、两套 clippy、fmt 与 diff 检查通过；真实桌面鼠标与候选窗未测。

## 下一步

目前没有另行批准的新实施阶段。先使用已交付的行为，按实际需求或问题确认后续最小范围；真实环境验证另行授权，不自动追加宿主矩阵。

更多 Magit 操作 / 完整 transient、脚本扩展 / async、LSP 或 Job 持久化均需另行确认。按需参考 [脚本扩展与热重载调研](work/scripting-extension-research.md) 和 [Magit 上游参照](work/f-magit-reference.md)，两者不额外授权其他范围。有效用户行为仍见[保真度契约](emacs-fidelity.md)，不会改写 F1 / F2 的历史排除范围。

## 已知问题与未排期事项

### 已接受的暂缓项与观察

这些事项尚未修复，不自动重开或追加宿主矩阵；若实际影响使用，再确认最小定向范围。

- **首次命令准备 draw 峰值**：small 三轮首次命令准备 draw 35.408–37.827 ms >16 ms，其中 prepaint 34.355–36.640 ms；后续 diff 动作没有相应超限。[静态调查](records/f1-command-prepaint-inspection.md)指向首次粗体候选的字体匹配冷路径，精确根因 / 占比未证实。用户接受为非阻塞观察；不与 E3 字体切换问题合并，不记作总体预算通过。以后测量遵循[冷 / 热路径规则](performance.md#适用原则)：同进程相关字体使用可能建立缓存，纯文本浏览不保证如此，重启不能沿用进程内缓存；不能静默预热或跳过首次峰值。[宿主证据](records/f1-diff-diagnostic-host-01.md)保留非验收、预算未通过的结论。
- **字体切换 draw 峰值**：单 mixed 完整诊断的字体阶段 draw 50.328 ms >16 ms、首场景 50.664 ms >50 ms；完整 ready 229.014 ms，常规滚动 / 输入正常。用户接受当前速度并暂缓定位；原预算及超限结论不变，不视为全部性能通过。见 [完整诊断](records/e3-display-diagnostic-host-01.md)。
- **中文输入轻微视觉跳动**：E3 宿主复核中同行英文约向下跳 1–2 px，用户接受暂不修复；尚未量化行高 / 基线变化或确认根因。见 [人工记录](records/e3-host-manual-02.md)。
- **自定义缺失字体族 fallback**：GPUI 固定 fallback 丢失请求字重 / 斜体，通用名 `monospace` 未作为系统别名解析。随包 Lilex 只解决默认字体路径，显式指定上述名称仍可能回退普通字体；跨平台别名或依赖修复另行确认。见 [解析诊断](records/e3-font-resolution-host-01.md)。
- **多 view 聚合性能观察**：六类 headless 双 view 全部 ready 曾约 350 ms；聚合没有独立数值门限，不能套用旧单 view / 选中 view 的 300 ms 预算判作超限。用户已接受该非阻塞观察；不外推两个原生代表样本为六类全覆盖。只有出现实际响应问题或确认新预算时再分析；历史边界见 [阶段 C 归档](archive/completed-through-f1.md#阶段-cbuffer--view-与-split)。

### 未排期需求

- **密码 / passphrase 交互（F3 之外）**：用户已确认排除于 F3；保留后续按 Magit 细化提示识别、保密读入（遮蔽与历史隔离）、取消及进程归属的需求。需先落实输入基础设施并确认实施范围，不使用普通文本读入冒充支持；该需求不属于已交付的 F3 范围。
- **任务持久化与恢复**：认可核心 Job、跨 reload 存活和分级恢复的方向，尚未批准实现或存储方案；具体待决项见[调研](work/scripting-extension-research.md#9-待确认事项)。先由实际任务推动，不能把自动重跑视为安全恢复。
- **鼠标多击 / Shift / 右键交互**：基础选区已交付；双击选词、三击选行（含行尾换行）、Shift 调整 region、右键 `mouse-save-then-kill` 与 `mouse-drag-copy-region`、拖动移动文本尚未实现。Emacs 31.1 的参照行为记录在[本阶段工作稿](work/mouse-region-file-prefill.md#已核实的上游参照)，实施前先按该工作稿确认范围。
- **日常编辑增强**：isearch、`M-y`、`C-u`、occur 等，实际需要时再确认优先级与范围。
- **自动保存与备份**：保留为数据安全增强需求；排期前先确认触发、写入位置与恢复行为。

## 完成标准

各阶段按已确认范围交付，执行 [本地检查](acceptance.md#本地检查与-headless-入口) 及按影响选择的回归；真实环境验证另行取得同意，未测不记通过。收尾时按 [维护检查点](README.md#三个维护检查点) 更新当前工作、保留重要遗留事项并归档必要证据，不继续在本文累积已完成阶段的过程和测试清单。
