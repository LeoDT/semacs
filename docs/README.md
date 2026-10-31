# semacs 文档

semacs 是一个用 Rust + [GPUI Kit](https://gpui-kit.com) 实现的 Emacs 风格桌面文本编辑器。
项目重点是**复刻 Emacs 的操作模型与按键逻辑**，而不是复刻 Emacs 的外观或扩展生态。
后续阶段会在同一套键位 / 命令 / 模式框架上增加 diff 查看器和 magit 风格的 git 操作界面。

当前状态：方案已讨论并定稿，尚未开始实现。

## 阅读顺序

1. [需求与范围](requirements.md) —— 项目要做什么、不做什么
2. [Emacs 保真度](emacs-fidelity.md) —— "像 Emacs 到什么程度"的判定标准与清单（本项目最重要的约束）
3. [架构设计](architecture.md) —— 分层、模块、按键分发链路、Buffer 边界
4. [决策记录](decisions.md) —— 已做出的技术选择、理由、被否方案、后果
5. [实施计划](plan.md) —— 待验证 spike、里程碑、验收标准

## 约定

- 文档中的 "Emacs" 指 GNU Emacs 31.1（本机实测版本），行为以它的可观察表现为准。
- 文档区分"已决定"与"待讨论 / 待验证"。未决项明确标注，不在实现中默认拍板。
- 按键写法沿用 Emacs：`C-` = control，`M-` = meta/alt，`S-` = shift，`s-` = super，多个键以空格分隔，例如 `C-x C-s`。
