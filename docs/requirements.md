# 需求与范围

## 1. 项目定位

semacs 将 Emacs 的操作模型带到现代 GPU UI：

- 输入采用 Emacs 键位与命令模型，包括前缀、`C-g`、`M-x`、kill ring、mark/region 与 minibuffer；
- 数据模型围绕 Buffer、window、mode 与 command；
- GPUI 提供统一窗口、平台输入/IME、字体 shaping 与绘制；gpui-kit 用于外围 UI 与主题组件。

后续阶段可在同一框架增加 diff 查看器与 magit 风格的 git 界面。

## 2. 当前状态

当前已有文件层、Rope / SumTree、自有 Buffer / 输入会话、正文视图与最小 GPUI 输入驱动，支持文本 / 预编辑输入、换行、逐字符删除、软换行、点击定位、滚动及 dirty 关闭确认。完整 Emacs 命令、undo 和保存尚未实现。headless 输入 / 几何回归、只读性能和首轮输入性能已有独立记录；只读与最小输入驱动 headless 预算均已确认，首轮真实 Wayland / fcitx5 功能验证已在软件渲染下完成，硬件 GPU / 原生性能和完整矩阵仍待完成，见 [原生输入记录](native-input.md)。详见 [当前落地状态](editor-plan.md#当前已落地部分)。

## 3. 目标（按优先级）

1. **按键与命令语义一致**：前缀、未定义键、取消与自插入行为以 Emacs 为准。
2. **可观察的编辑行为一致**：point / mark、region、kill ring、motion、undo 边界与 minibuffer 流程以 Emacs 为准。
3. **日常文本编辑可用**：打开、编辑、保存与切换 buffer，基础操作可纯键盘完成。
4. **为后续能力留出清晰边界**：mode keymap、只读 buffer、transient keymap 与 section 显示。

## 4. 核心原则

- **键盘优先**：v1 基础编辑可纯键盘完成。
- **语义按 Emacs，外观按 GPUI**：不模拟 Emacs 的 GTK / 终端外观或 GUI 控件。
- **不做 elisp**：命令用 Rust 实现；不实现 elisp 解释器或 Emacs 包生态。
- **单顶层窗口**：v1 一个 OS 窗口、一个正文 view；不实现 split。
- **明确所有权**：Buffer、View、输入会话与命令路由由 semacs 控制；所有已提交文本变更经过唯一 Edit 入口。
- **KISS / YAGNI**：未列入 v1 的功能不提前实现。

## 5. v1 范围

包含：

- 启动时打开命令行参数指定的文件，或创建 scratch buffer；
- 单顶层窗口、一个正文 view、多个可切换 buffer；mode line 与 echo area；
- Emacs 基本 motion、编辑、kill ring、mark/region 与 undo；
- `M-x`、`C-x C-f`、`C-x C-s`、`C-x C-w`、`C-x b`、`C-x k`、`C-x C-c`；
- UTF-8 有/无 BOM、LF/CRLF 文件读写，dirty 标记与退出确认；
- 键位引擎、命令注册表与 major mode 框架；
- 键位、文本变换、真实输入链路与 Emacs 基准测试。

不包含：

- isearch、正则替换、rectangle、键盘宏、narrowing、间接 buffer；
- window split、多 frame、tab-bar；
- 用户配置系统与自定义键位文件；
- 语法高亮、LSP、正文代码补全（minibuffer 基础补全包含在 v1）；
- 插件/脚本扩展；
- diff 与 git（后续阶段）。

v1 阶段为 R0、R1 后的 M1–M3；M4/M5 保持为后续阶段。`C-u`、`M-y`、isearch 与 split 不作为 v1 发布条件。

## 6. 技术与组件约束

- Rust 2024 edition，单 crate（`semacs`）。
- GPUI 是统一窗口、平台输入/IME 协议、字体 shaping 与绘制基础；gpui-kit 仅承担外围 UI 与主题组件，不承担正文或 minibuffer 编辑模型。
- semacs 自有 Buffer、View、输入会话、命令集成和 Emacs 编辑语义。预编辑是独立临时状态；已提交变更通过唯一 Edit 入口；异步布局/输入回调绑定目标及版本或会话代次。
- 首批 Zed Rope 与 SumTree 源码组件已在单 crate 中导入，来源 / 许可见 [THIRD_PARTY.md](../THIRD_PARTY.md)。R1 依 [editor-plan.md](editor-plan.md) 完成纵向切片；不依赖 Zed 运行时，不维护外部 fork。
- D21 确立自有 editor 路线；D22 定义源码复用与许可证要求。GPL-3.0-or-later 可接受；须记录来源、版本、许可证及本地修改，不宣称整体产品可按 MIT 发布。
- 平台：Linux 优先，Wayland 为验收环境；X11 基础可用是目标，不代表已验证。具体平台验收以 editor-plan 的门禁为准。
- 键位：纯 Emacs，不采用 CUA `Ctrl-C / Ctrl-V`；复制/粘贴走 `M-w` / `C-y`。仅保留窗口管理层面的系统键。

## 7. 阶段与验收

正式顺序见 [plan.md](plan.md)，组件与验收细节见 [editor-plan.md](editor-plan.md)：

- **R0 文档与清理**：明确模块职责、组件来源及许可证；本阶段保持应用外壳与文件能力，不提供正文编辑。
- **R1 最小正式切片**：验证 GPUI 输入协议、预编辑/提交/取消、目标与会话生命周期、布局及输入几何；建立可复现的大文件打开、滚动、输入响应和布局性能基线。完整 Emacs undo/history 算法不是 R1 的门槛，安排在 M1。
- **M1–M3**：完成 v1 编辑、history、文件、minibuffer、交互和完整 Emacs fidelity 验收。M3 汇总全表行为验收。

v1 验收要求：

- 无鼠标完成打开文件 → 编辑 → 保存 → 切换 buffer → 退出；每项均有 Emacs 键位。
- 前缀与 `C-g` 行为正确；未定义键不插入字符并在 echo area 报错。
- 中文/IME 输入正确；预编辑期间的命令/前缀按 D17 拒绝并提示，关闭/失焦按 D19 取消预编辑；不保存临时文本，不把迟到回调写入新目标，pending 前缀不被文本输入绕过。
- point、mark、region、kill ring 与 minibuffer 行为符合 [Emacs 保真度](emacs-fidelity.md)；A → B → A 状态不串用；minibuffer 与正文共享 kill ring、undo 独立且 prompt 不可编辑。
- 按 D20 无损读写 UTF-8 有/无 BOM、统一 LF/CRLF；无效 UTF-8 与不支持编码明确报错，不破坏原文件。保存失败不清 dirty；modified/save-point 遵循 D16。
- 退出、kill dirty buffer 与取消确认均有验收；完整保真度验收在 M3 完成。

## 8. 后续阶段

1. **diff 查看器**：评估可复用的 UI / diff 组件与只读文本显示，使用自己的 mode keymap；不预设 gpui-kit diff 编辑控件作为正文编辑视图。
2. **magit 风格 git**：只读 buffer、section 显示与 transient keymap（如 `?` 帮助、`s` stage、`c` commit）。
3. 可选增强：isearch / occur、语法高亮、用户键位配置、多窗口与同 buffer 多 view。
