# semacs 文档

semacs 用 Rust 与 GPUI 实现 Emacs 的操作模型和按键逻辑，不复刻 Emacs 外观或 elisp 生态。
gpui-kit 用于外围 UI；文本模型、编辑视图、输入适配和命令语义由 semacs 掌控，通用底层选择性复用 Zed 代码。

## 当前状态

当前提供文件 / scratch 的正文视图、最小文本输入、软换行、光标、点击定位及横纵滚动；mode line 显示文件格式、dirty 和行列。
正文使用自有 Buffer / View 与 Zed Rope / SumTree，排版与绘制调用当前 GPUI；换行索引有有界预览和后台补全。
文件层支持 UTF-8 有 / 无 BOM、统一 LF / CRLF 的读取与编解码。

已有版本化 Edit、独立预编辑、目标绑定的 GPUI 输入 handler、UTF-16 几何与会话失效保护。
最小驱动支持输入、换行和逐 Unicode scalar 删除；预编辑有下划线，已有 dirty 的窗口关闭须确认。
**完整 Emacs keymap / 命令、undo 和保存尚未实现**，echo area 明示此限制；不采用 CUA 复制 / 粘贴。

已建立 GPUI headless 输入 / 视图回归与显示 / 输入性能入口；测试字体 / 无 GPU 的结果不能代表原生字体、真实 Wayland 或 IME 通过。
headless 与宿主硬件路径的原生 CPU / 提交流程测量及预算已确认；真实 Wayland / fcitx5 功能证据与硬件结果分别记录于 [原生输入记录](native-input.md)。**R1 已按用户确认的验收边界完成，下一阶段为 M1**；GPU 执行 / 实际呈现时间仍明确未测，不宣称已通过这些指标。来源与许可见 [THIRD_PARTY.md](../THIRD_PARTY.md) 和 [LICENSE](../LICENSE)。

## 阅读顺序

1. [需求与范围](requirements.md)：目标、当前能力和 v1 边界
2. [Emacs 保真度](emacs-fidelity.md)：行为基准、差异分级与验收矩阵
3. [架构设计](architecture.md)：状态归属、编辑入口、视图和输入生命周期
4. [技术决策](decisions.md)：当前生效的实现与交互选择
5. [实施计划](plan.md)：R0 / R1 与 M1–M3 交付门禁
6. [编辑器实现计划](editor-plan.md)：源码组件、许可、最小切片验收
7. [性能基线与测量](performance.md)：样本、文件层结果及编辑视图测量要求
8. [原生输入记录](native-input.md)：实际环境、已验证场景与剩余限制
9. [Emacs 对照基准](../tests/emacs/README.md)：独立参照脚本及使用边界

## 约定

- Emacs 指 GNU Emacs 31.1；对照固定 major mode、选项、初始状态与 command loop 驱动方式。
- 需求定义目标，架构定义设计，计划定义验收；未实现或未测量的内容不写作已通过。
- 新增保真度差异、平台依赖调整和输入策略变化须先确认。用户真实环境验证必须另行征求同意。
- 按键记法为 `C-`、`M-`、`S-`、`s-`，多键序列以空格分隔，如 `C-x C-s`。
