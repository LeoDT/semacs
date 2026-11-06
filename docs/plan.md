# 实施计划

本文是当前工作和跨会话待办的唯一入口；维护与归档规则见 [文档说明](README.md#维护规则)。范围见 [需求](requirements.md)，行为见 [保真度契约](emacs-fidelity.md)。里程碑按可验收行为划分，不按工期估算。

## 当前状态：F2 已完成，下一阶段待确认

R0–M3、A–E3、日常编辑增强及 F1 只读 Magit diff 已按各自确认范围完成，见 [R0–F1 归档](archive/completed-through-f1.md)。F2 status / 单文件 stage / unstage 也已按批准范围完成，见 [F2 归档](archive/f2-status.md)；具体本地 / 宿主证据见 [冻结记录索引](records/README.md)。

用户确认现有 headless 基线、隔离自动化和单窗口宿主功能证据足以收尾 F2，不追加原生性能、GPU 或 IME 测试。status / 写入端到端仍只记基线，不表示任意仓库的时延保证，也不宣布所有原生性能预算通过；下列历史暂缓项与原预算不变。F2 没有已知阻塞性遗留事项。

## 下一步：按实际需求确认范围

当前没有启动新的实施阶段。F2 的长期边界见 [行为契约](emacs-fidelity.md#阶段-f2status-与单文件-index-操作)、[D26](decisions.md#d26-rust-核心与脚本扩展)、[D28](decisions.md#d28-buffer-mode-与结构化显示)及 [JS 配置](scripting.md)。

后续更多 Magit 操作 / transient、脚本扩展 / async、LSP 或 Job 持久化都需另行确认范围与依赖，不因 F2 完成自动开放。可按需参考 [脚本扩展与热重载调研](work/scripting-extension-research.md) 和 [Magit 上游参照](work/f-magit-reference.md)，两者不是新阶段的实施授权。

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
