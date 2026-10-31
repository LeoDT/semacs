# semacs 文档

semacs 是一个用 Rust + [GPUI Kit](https://gpui-kit.com) 实现的 Emacs 风格桌面文本编辑器。
项目重点是**复刻 Emacs 的操作模型与按键逻辑**，而不是复刻 Emacs 的外观或扩展生态。
后续阶段会在同一套键位 / 命令 / 模式框架上增加 diff 查看器和 magit 风格的 git 操作界面。

当前状态：尚未开始正式实现。S1–S5 的机制验证已完成并落在 `spikes/`：自建 undo（D16）、
IME 命令屏障与关闭 / 失焦取消（D17）、mark marker 语义（D18）均已实测，
真实 fcitx5 的关闭、失焦与提交后 mark 由 real-cases 覆盖，redo point 已对齐 Emacs。
剩余为可选的真实硬件键盘 / WM 保留键确认；这些完成前不宣布 S1–S5 全部通过，也不进入 M0。
v1 截止 M3，split 与 diff / git 不在 v1 内。

## 阅读顺序

1. [需求与范围](requirements.md) —— 项目要做什么、不做什么
2. [Emacs 保真度](emacs-fidelity.md) —— "像 Emacs 到什么程度"的判定标准与清单（本项目最重要的约束）
3. [架构设计](architecture.md) —— 分层、模块、按键分发链路、Buffer 边界
4. [决策记录](decisions.md) —— 已做出的技术选择、理由、被否方案、后果
5. [实施计划](plan.md) —— 待验证 spike、里程碑、验收标准

## 约定

- 文档中的 "Emacs" 指 GNU Emacs 31.1，行为以它的可观察表现为准；对照测试需固定 major mode、选项与脚本。
- 需求 / 保真度定义目标，架构定义候选实现，计划定义验收；参考源码和旧对照记录不代表运行验证通过。
- 文档区分"已决定"与"待讨论 / 待验证"。未决项明确标注，不在实现中默认拍板。
  历史决策的修正见 D14 / D15；新增保真度差异、上游 API 修改或依赖 patch 均须另行确认。
- 按键写法沿用 Emacs：`C-` = control，`M-` = meta/alt，`S-` = shift，`s-` = super，多个键以空格分隔，例如 `C-x C-s`。
