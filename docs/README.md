# semacs 文档

semacs 用 Rust 与 GPUI 实现 Emacs 的操作模型和按键逻辑，不复刻 Emacs 外观或 elisp 生态。
gpui-kit 用于外围 UI；文本模型、编辑视图、输入适配和命令语义由 semacs 掌控，通用底层选择性复用 Zed 代码。

## 当前状态

当前代码提供应用窗口外壳、文件参数 / scratch、文件格式元数据、mode line 和 echo area。
文件层已支持 UTF-8 有 / 无 BOM、统一 LF / CRLF 的读取与编解码。
**正文显示与编辑尚未实现**，窗口显示明确的占位提示；当前不提供保存、dirty、IME 编辑或 Emacs 命令。
Zed Rope / SumTree 已选为首批源码组件，尚未导入。下一步是 R1 最小正式编辑切片。

## 阅读顺序

1. [需求与范围](requirements.md)：目标、当前能力和 v1 边界
2. [Emacs 保真度](emacs-fidelity.md)：行为基准、差异分级与验收矩阵
3. [架构设计](architecture.md)：状态归属、编辑入口、视图和输入生命周期
4. [技术决策](decisions.md)：当前生效的实现与交互选择
5. [实施计划](plan.md)：R0 / R1 与 M1–M3 交付门禁
6. [编辑器实现计划](editor-plan.md)：源码组件、许可、最小切片验收
7. [性能基线与测量](performance.md)：样本、文件层结果及编辑视图测量要求
8. [Emacs 对照基准](../tests/emacs/README.md)：独立参照脚本及使用边界

## 约定

- Emacs 指 GNU Emacs 31.1；对照固定 major mode、选项、初始状态与 command loop 驱动方式。
- 需求定义目标，架构定义设计，计划定义验收；未实现或未测量的内容不写作已通过。
- 新增保真度差异、平台依赖调整和输入策略变化须先确认。用户真实环境验证必须另行征求同意。
- 按键记法为 `C-`、`M-`、`S-`、`s-`，多键序列以空格分隔，如 `C-x C-s`。
