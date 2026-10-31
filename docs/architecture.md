# 架构设计

## 当前状态与目标

当前产品是 GPUI 应用外壳和文件层：窗口与外围界面可运行，文件读取及编码/换行格式处理已完成；正文编辑能力尚未提供。本轮目标是为新 editor 路线定义设计，不代表正文编辑已实现。

产品使用 GPUI 作为统一窗口、平台输入、IME 协议、字体 shaping 与绘制基础；gpui-kit 只承担外围 UI 与主题组件。正文与 minibuffer 编辑模型、命令集成和 Emacs 语义由 semacs 自有实现。

D21 确立自有 editor 路线，D22 定义源码复用与许可要求。首批已选择 Zed 的 Rope 与 SumTree 作为源码组件，尚未导入。可接受 GPL-3.0-or-later 组件；源码复用须在 D22 中记录来源、版本、许可证及本地修改，不宣称整体产品可按 MIT 发布。不依赖 Zed 运行时，不维护外部 fork。

组件选择、模块划分与分阶段验收详见 [editor-plan.md](editor-plan.md)。

## 1. 职责边界

```text
GPUI / 平台
  ├─ 窗口、平台输入协议、IME、字体 shaping、绘制
  └─ gpui-kit 外围 UI 与主题组件

semacs
  ├─ AppShell：窗口应用状态、焦点与命令出口
  ├─ keys / command：Emacs 键位解析与命令注册
  ├─ Buffer：已提交文本、point / mark、文件与 dirty 状态
  ├─ View：viewport、scroll、布局、输入几何与显示
  ├─ InputSession：目标绑定的临时预编辑状态
  └─ minibuffer：独立读入会话与补全
```

保持少量职责清晰的模块，避免重复的 editor / buffer 抽象层、插件机制、可插拔后端或并行实现。命令只通过自有模型接口修改文本，不依赖 UI 组件内部状态。

## 2. 文本、视图与输入状态

- **Buffer** 持有已提交文本、文件身份与格式、dirty / 保存点、undo history 及 mark。v1 为单正文 view；point 与 goal column 可暂存于 Buffer，但不据此宣称实现了多 view 的 window-point。
- **View** 持有 viewport、scroll、布局、可见区域及输入几何。未来若增加 view，须另行定义窗口级 point 与滚动语义；v1 不提供 split。
- **预编辑**是绑定到目标和会话代次的独立临时状态。更新与取消不污染已提交正文、mark、dirty 或 undo history；提交才进入已提交文本。
- **唯一 Edit 入口**接收目标、范围、替换内容及来源，按序更新已提交文本与相关语义状态。键盘编辑、命令、自插入、IME 提交和补全均经此入口；undo/redo 也通过同一入口回放。R1 验证最小闭环；完整 undo/history 算法在 M1 实现。
- 异步布局结果与输入回调关联目标及版本/会话代次。目标或布局版本失效后丢弃或显式取消，不按当前焦点重新解释迟到结果。
- region 的显示属于 View；point、mark 与激活状态以自有语义模型为准。

## 3. 键盘与命令

平台输入层处理焦点、系统保留键、死键、AltGr、IME 与文本分类，再将键事件交给纯逻辑 Emacs 键位引擎。命令统一经 AppShell 选择当前目标；正文与 minibuffer 共用已批准的编辑语义和 kill ring，但两者 undo history 独立，prompt 不可编辑。

已批准的键位、编辑、region、IME 和退出行为见 [Emacs 保真度](emacs-fidelity.md)。GPUI 的作用是平台与 UI 能力，不替代 semacs 的命令分发或编辑语义。

## 4. IME 与生命周期

预编辑和已提交文本分离。保留 D17：组合活动时拒绝命令和前缀并提示；保留 D19：窗口关闭或失焦时取消未提交组合。输入会话绑定原目标和代次，迟到提交不得写入其他 Buffer 或新的 minibuffer 会话。保存只读取已提交文本。

R1 对 GPUI 输入协议、预编辑/提交/取消、目标归属、布局与输入几何以及迟到事件进行真实集成验收。布局与输入几何使用版本化结果；具体实现由纵向切片验证，不预设特定回调补丁。

## 5. 语义与产品范围

保留已批准的 D16–D20 语义：D16 自有 undo 与 modified/save-point 规则；D17 组合期间拒绝命令；D18 mark 的 marker 行为；D19 关闭/失焦取消；D20 UTF-8 BOM 及 LF/CRLF 文件往返。实现以 Emacs 31.1 基准及 [F1/F2/F3](emacs-fidelity.md) 为验收约束。新能力不自动改变这些策略。

v1 使用单顶层窗口和一个正文 view，不提供 split；不实现 narrowing。其他发布范围见 [requirements.md](requirements.md)。

## 6. 目录建议

目录以实现需要为准，优先少量模块，不要求一项职责对应一个 crate：

```text
src/
  app.rs          应用外壳、焦点与命令出口
  platform.rs     GPUI 平台输入、IME 与平台差异
  keys.rs         键位解析
  command.rs      命令注册
  buffer.rs       已提交文本、编辑状态与文件关联
  view.rs         布局、输入几何与显示
  input.rs        输入会话与临时预编辑
  minibuffer.rs   读入会话与补全
```

R0 记录源码组件和许可；R1 完成可运行的最小正式纵向切片，并验收 GPUI 输入/几何与性能基线。M1 实现完整编辑和 history 算法；M1–M3 完成 v1 功能与保真度验收。详细交付物及门禁以 [editor-plan.md](editor-plan.md) 和 [plan.md](plan.md) 为准。
