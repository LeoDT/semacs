# 实施计划

本文是当前工作和跨会话待办的唯一入口；维护与归档规则见 [文档说明](README.md#维护规则)。范围见 [需求](requirements.md)，行为见 [保真度契约](emacs-fidelity.md)。里程碑按可验收行为划分，不按工期估算。

## 当前状态：基础阶段已归档，下一实现范围待确认

R0–M3、A–E3、日常编辑增强及 F1 只读 Magit diff 已按各自确认范围完成；详细结论和历史测试数量见 [已完成阶段归档](archive/completed-through-f1.md)，批次证据见 [冻结记录索引](records/README.md)。归档不表示所有性能预算、真实 IME 或 GPU / 实际呈现均已通过；下列暂缓项和原预算保持不变。

本轮文档整理已完成，未开始 Magit、脚本 API 或任务系统实现。下一步先确认脚本扩展边界与首个 Magit 切片，再形成实施规格；不自动进入 Git 写操作、完整 Magit、LSP 或完整 Durable workflow。

## 下一步：脚本扩展边界与 Magit 切片

现有有效边界仍见 [D26](decisions.md#d26-rust-核心与脚本扩展)、[D28](decisions.md#d28-buffer-mode-与结构化显示)及 [JS 配置](scripting.md)。后续设计材料：

- [脚本扩展与热重载调研](work/scripting-extension-research.md)：现状、Chord 参照、Magit / LSP 能力分析，以及 [Job 与恢复的讨论共识](work/scripting-extension-research.md#8-job-与可恢复任务讨论共识及实施边界)。方向已讨论，具体 API、会话归属、重载行为矩阵及实施范围仍需确认。
- [Magit 上游行为参考](work/f-magit-reference.md)：固定外部参照，不是已批准的 semacs 规格。

建议下一次定范围时依次确认：

1. **脚本与核心边界**：哪些状态随脚本代次失效、哪些任务 / 会话由核心持有；必要查询、操作请求、取消、限额与迟到结果规则。
2. **首个可验收切片**：候选为最小 JS 工作流能力加只读 status，复用已有 diff / section；尚未批准具体功能清单，不先开放所有扩展原语。
3. **后续依赖**：Git 写操作与 transient 另定范围；核心 Job 按实际异步工作需求引入，跨重启持久化和完整 Durable workflow 不作为 Magit 前置条件。

以上是后续设计入口，不是实现排期或测试授权。

## 已知问题与未排期事项

### 已接受的暂缓项与观察

这些事项尚未修复，不自动重开或追加宿主矩阵；若实际影响使用，再确认最小定向范围。

- **首次命令准备 draw 峰值**：small 三轮首次命令准备 draw 35.408–37.827 ms >16 ms，其中 prepaint 34.355–36.640 ms；后续 diff 动作没有相应超限。[静态调查](records/f1-command-prepaint-inspection.md)指向首次粗体候选的字体匹配冷路径，精确根因 / 占比未证实。用户接受为非阻塞观察；不与 E3 字体切换问题合并，不记作总体预算通过。以后测量遵循[冷 / 热路径规则](performance.md#适用原则)：同进程相关字体使用可能建立缓存，纯文本浏览不保证如此，重启不能沿用进程内缓存；不能静默预热或跳过首次峰值。[宿主证据](records/f1-diff-diagnostic-host-01.md)保留非验收、预算未通过的结论。
- **字体切换 draw 峰值**：单 mixed 完整诊断的字体阶段 draw 50.328 ms >16 ms、首场景 50.664 ms >50 ms；完整 ready 229.014 ms，常规滚动 / 输入正常。用户接受当前速度并暂缓定位；原预算及超限结论不变，不视为全部性能通过。见 [完整诊断](records/e3-display-diagnostic-host-01.md)。
- **中文输入轻微视觉跳动**：E3 宿主复核中同行英文约向下跳 1–2 px，用户接受暂不修复；尚未量化行高 / 基线变化或确认根因。见 [人工记录](records/e3-host-manual-02.md)。
- **自定义缺失字体族 fallback**：GPUI 固定 fallback 丢失请求字重 / 斜体，通用名 `monospace` 未作为系统别名解析。随包 Lilex 只解决默认字体路径，显式指定上述名称仍可能回退普通字体；跨平台别名或依赖修复另行确认。见 [解析诊断](records/e3-font-resolution-host-01.md)。
- **多 view 聚合性能观察**：六类 headless 双 view 全部 ready 曾约 350 ms；聚合没有独立数值门限，不能套用旧单 view / 选中 view 的 300 ms 预算判作超限。用户已接受该非阻塞观察；不外推两个原生代表样本为六类全覆盖。只有出现实际响应问题或确认新预算时再分析；历史边界见 [阶段 C 归档](archive/completed-through-f1.md#阶段-cbuffer--view-与-split)。

### 未排期需求

- **任务持久化与恢复**：认可核心 Job、跨 reload 存活和分级恢复的方向，尚未批准实现或存储方案；具体待决项见[调研](work/scripting-extension-research.md#9-待确认事项)。先由实际任务推动，不能把自动重跑视为安全恢复。
- **日常编辑增强**：isearch、`M-y`、`C-u`、occur 等，实际需要时再确认优先级与范围。
- **自动保存与备份**：保留为数据安全增强需求；排期前先确认触发、写入位置与恢复行为。

## 完成标准

各阶段按已确认范围交付，执行 [本地检查](acceptance.md#本地检查与-headless-入口) 及按影响选择的回归；真实环境验证另行取得同意，未测不记通过。收尾时按 [维护检查点](README.md#三个维护检查点) 更新当前工作、保留重要遗留事项并归档必要证据，不继续在本文累积已完成阶段的过程和测试清单。
