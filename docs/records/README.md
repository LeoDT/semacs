# 冻结记录索引

本目录保存固定版本下的验收与诊断证据，包括失败、修复、用户确认及未测边界；不是当前待办，也不表示后续代码自动通过。先从 [实施计划](../plan.md) 确认当前工作，需要历史上下文时再查 [R0–F1 归档](../archive/completed-through-f1.md)、[F2 归档](../archive/f2-status.md)、[F3 归档](../archive/f3-window-commit.md)和对应批次。

记录正文不随当前实现更新，后续复测另记；允许修复引用，不改写历史结论。维护规则见 [文档说明](../README.md#冻结证据)。本索引随新增记录维护，不复制测试数量、完整批次经过或当前问题清单。

| 材料 | 用途与边界 |
|---|---|
| [Magit 样式、M-x 历史与 scratch 退出本地验证](magit-style-history-scratch-local.md) | 三项插入需求的本地回归、集中审查修复及获准重跑后的 status / diff headless 基线；不表示真实窗口观感或原生性能预算通过。 |
| [F3 临时 window / 普通 commit 本地验证](f3-window-commit-local.md) / [单窗口宿主工具准备](f3-host-tools-local.md) / [单窗口宿主功能验证](f3-commit-host-01.md) | 隔离真实 Git / helper、headless 生命周期、阶段集中审查修复后的本地检查，以及单窗口人工观察和独立 Git / 消息字节核对；不包含密码 / passphrase 读入或性能矩阵。 |
| [F2 status / 单文件写入本地验证与基线](f2-status-local.md) / [单窗口宿主工具准备](f2-host-tools-local.md) / [单窗口宿主功能验证](f2-status-host-01.md) | 隔离自动化、64 / 4096 条清单 headless 基线、工具自检，以及一窗口 status / 默认写键 / 过时标识与完整 index 检查点；不表示完整平台或性能预算通过。 |
| [F1 diff 专项本地基线](f1-diff-performance-local.md) / [首批启动失败与修复](f1-diff-performance-host-01.md) / [第二批 setup 超限](f1-diff-performance-host-02.md) / [完整诊断工具](f1-diff-diagnostic-local.md) / [三轮宿主完整数据](f1-diff-diagnostic-host-01.md) | 两档 headless 基线、两次原生失败、完整诊断工具及 small 三轮宿主数据；首次命令准备峰值另见 [静态调查](f1-command-prepaint-inspection.md)。诊断不是总体预算通过。 |
| [F1 mode 与 Magit diff 本地验证](f1-magit-diff-local.md) / [宿主定向验收](f1-magit-diff-host-01.md) | mode / 局部键位、只读 diff、本地回归及四组宿主功能证据。 |
| [E2 多语言本地验证](e2-languages-local.md) / [宿主工具准备](e2-host-tools-local.md) / [首批宿主回传](e2-languages-host-01.md) / [反馈修复本地验证](e2-flicker-fix-local.md) / [第二批宿主复核](e2-flicker-host-02.md) | 多语言接入、宿主工具、首批失败、反馈修复及第二批复核。 |
| [E3 本地实现与性能回归](e3-styles-local.md) / [首版性能失败](e3-typography-probe.md) | 字体 / 装饰与背景的初期实现、首版性能失败；最终收尾边界见 [E3 归档](../archive/completed-through-f1.md#阶段-e3文本样式与高亮合成)。 |
| [E3 首批宿主字体失败](e3-host-manual-01.md) / [离线 fallback 诊断](e3-font-fallback-local.md) / [系统字体解析诊断](e3-font-resolution-host-01.md) | 默认 monospace / 字体 fallback 的失败与诊断，不用后续修复覆盖旧结果。 |
| [E3 宿主工具准备](e3-host-tools-local.md) | 人工 / 性能入口准备及离线检查，不是宿主结果。 |
| [E3 随包 Lilex 本地修复](e3-bundled-font-local.md) / [第二批宿主人工复核](e3-host-manual-02.md) | 默认随包字体修复和人工复核；不表示自定义 fallback 或中文输入跳动已修复。 |
| [E3 随包字体首批原生性能失败](e3-native-perf-host-01.md) / [离线换行成本分解](e3-wrap-cost-local.md) / [换行与及时停止本地修复](e3-wrap-fix-local.md) / [单窗口原生复测](e3-native-perf-host-02.md) | 完整布局过慢、换行成本分析、修复及超限即停的原生复测。 |
| [单窗口完整显示诊断工具](e3-display-diagnostic-local.md) / [宿主完整数据](e3-display-diagnostic-host-01.md) | 完整显示诊断的工具与宿主数据；单轮非验收，保留字体切换峰值。 |
| [M2 profiler 生命周期工具修复](profiler-lifecycle-local.md) | 消息历史接入后修正旧生命周期断言；未运行原生矩阵。 |
| [Rust 语法本地验证](e1-syntax-local.md) / [宿主定向验收](e1-syntax-host.md) | Rust 语法接入的本地回归与定向宿主观察。 |
| [基础主题本地验证](e0-themes-local.md) / [宿主定向验收](e0-themes-host.md) | 主题接入的本地 / 宿主证据，保留当时 profiler 工具失败。 |
| [消息显示与历史本地验证](d1-messages-local.md) / [宿主定向验收](d1-messages-host.md) | 消息显示 / 历史的本地 / 宿主证据。 |
| [最小脚本接入本地验证](d-scripting-local.md) / [宿主冒烟](d-scripting-host.md) | 最小配置 / region 变换证据，保留修复前消息重叠反馈。 |
| [候选式 minibuffer 本地验证](b-minibuffer-local.md) / [宿主验收](b-minibuffer-host.md) | 候选式读入的本地基线、用户观察及覆盖边界。 |
| [buffer / view 与 split 本地验证](c-split-local.md) / [验收自动化与多 view 基线](c-automation-local.md) / [真实 IME 宿主验收](c-split-host.md) | 共享 buffer / view、自动化与真实 IME 的固定证据。 |
| [多 view 原生性能工具本地验证](c-native-performance-local.md) / [宿主验证](c-native-performance-host.md) | 两方向多 view 原生性能工具与代表性宿主数据。 |
| [显示设置验收](a-display-local.md) | 显示设置的本地、人工与宿主性能证据。 |
| [M2 / M3 验收](m2-m3-local.md) | 文件 / buffer 阶段验收、首屏诊断与修复。 |
| [M1 性能验收](m1-performance.md) / [R1 验收](r1.md) | 基础编辑与性能的固定环境、实测和限制。 |
