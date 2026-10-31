# 编辑器实现计划

状态：R1 已完成首批源码导入、Buffer / InputSession、正文 View 与最小 GPUI 平台输入 / 生命周期接线；已完成首轮真实 Wayland / Pinyin 功能验证（软件渲染），硬件 GPU 与完整性能验收仍未完成。
下述清单仍是整个 R1 的验收要求，不是编辑能力或性能已经通过的证明。

## 当前已落地部分

- `src/text/`：冻结快照的 Rope / SumTree、相关测试及最小工具函数；单 crate，无 Zed 构建路径。
  完整来源、修改和许可记录见 [THIRD_PARTY.md](../THIRD_PARTY.md)，项目许可见 [LICENSE](../LICENSE)。
- `src/buffer.rs`：文件 / scratch 的权威正文、UTF-8 字节 point、格式元数据、目标 / 文本版本、唯一 Edit 入口及最小 dirty。
  空插入不产生编辑；非空替换即使内容相同也推进版本，不以内容比较推断保存点。
- `src/input.rs`：纯输入会话、独立 Rope 预编辑投影、UTF-16 查询 / 范围调整、提交 / 取消及命令屏障接口。
  普通 point 预编辑不动已提交正文、point 或 dirty；平台显式替换删除正文时立即经 Edit 生效，取消不回滚。
  部分 marked 被替换后，未再 marked 的文字进入 committed；最终提交只插入剩余 marked 文本。
  同长度更新推进显示身份；清空 / 取消 / 组合提交更换 token，结束会话拒绝旧 token。
  输入校验绑定的正文版本；外部编辑之后必须建立新会话，结束后仍可读取 committed 显示快照。
  InputUpdate 分别提供 committed Edit 与精确投影 DisplayChange；连续 delta 可合成，取消仅移除当前 marked 范围。
- `src/display.rs`：流式 extended-grapheme 贪心换行、tab 间距、SumTree 字节 / 显示行索引。
  WrapJob 接收明确的前后显示身份与替换范围，重排从已有显示行检查点开始；边界和 tab phase 收敛后共享未变后缀。
  前台预览最多扫描 8 KiB（计入 Unicode lookbehind），不发布未完成行；其余由后台完成，临时 / 完整状态明确区分。
  布局 key 包含正文 / 预编辑身份、宽度、字体及特性、字号、行高、tab 参数；跨目标或过期 patch 不复用旧索引。
- `src/view.rs`：BufferView 持有 Buffer / InputSession、viewport / 滚动、可见行 shaping 缓存与几何；AppShell 观察它并提供外围 UI。
  可显示正文 / 空文档，点击定位光标、横纵滚动，支持宽度和字体参数变更。行列按 Unicode scalar 计列，不按 UTF-8 字节或 grapheme 计列。
  后台任务有取消标记、弱目标和结果身份校验；稳定帧共享可见 ShapedRow。单条可见行超过 8 KiB 时 shaping 转后台。
- `tests/buffer_input.rs`：坐标、纯模型和随机投影替换回归，含生产 chunk 配置下的 8 MiB 单行及混合正文。
- `src/platform.rs`：GPUI InputHandler 绑定弱目标、会话 token 和绘制注册代次，提供 UTF-16 文本 / 范围 / 命中 / 几何查询。
  同一 handler 只在自身成功回调后推进 token；新绘制、失焦 / 关闭失效旧 handler，不把旧事件改贴新目标。
- View 将真实投影 delta 传入 WrapJob，多次回调可合成并在后台未完成时保留原完整索引。
  预编辑有下划线，caret 使用投影 selection；新偏移不能读取旧布局，临时 / 最终可见布局就绪后刷新候选坐标，稳定帧不重复刷新。
- 最小驱动支持普通输入、换行、前 / 后逐 Unicode scalar 删除；命令尝试受 D17 屏障，无完整 keymap 或前缀系统，不引入 CUA 粘贴。
  AppShell 在 dirty 窗口关闭时提供 Cancel / Discard，取消后恢复新输入会话与焦点；仍未实现保存。
  `src/prompt.rs` 通过 GPUI PromptHandle 和 gpui-kit 按钮提供键盘可操作的确认框；统一由 `app::init` 初始化。
- `tests/display.rs`：InputSession 真实投影 delta / 合成 delta 驱动 WrapJob，覆盖部分提交、取消、显式组合提交与过期 after 身份。
- `src/view/input_tests.rs`：注册输入、同帧连续协议回调、跨帧旧 handler、D17、失焦、关闭确认、几何空窗 / 刷新及真实输入增量重排。
- `tests/display.rs`、`tests/view.rs`、view 内部测试：GPUI headless 下覆盖换行 / 合行 patch、grapheme 跨 Rope 块、tab / EOF、滚动、非零原点、改宽 / 字体、任务取消和空帧缓存共享。
- `examples/perf_view.rs`：六类约 8 MiB 正文的 TestPlatform / TestTextSystem 显示与插入 / 删除 / 换行 / 合行测量，数据与边界见 [性能记录](performance.md)。
- `examples/native_input.rs`：生产 AppShell 的显式原生诊断入口；真实 Wayland / fcitx5 结果及软件渲染限制见 [原生输入记录](native-input.md)。

尚未完成：完整 Emacs 命令 / keymap / history、保存（按 M1 / M2 推进）、原生字体 / 硬件 GPU / IME 性能及完整原生验证矩阵。
headless 协议测试使用与注册 handler 相同的适配器，但不模拟原生 IME 事件翻译；几何刷新记录的是调度请求，不是原生候选窗位置。
只读与最小输入驱动 headless 预算均已由用户确认，见性能记录；原生预算独立管理；本轮独立窗口测试已获授权并留证，不扩大授权范围，R1 保持未完成。

### 已确认的取消语义

默认 Emacs 输入在 point，不将 region 当作平台替换选区。
用户明确接受：平台正文替换已经发生就不因取消恢复；丢弃仍 marked 文本与 unmark 保留文字分别处理。
此前将非空替换回滚视为接线阻塞的判断撤销；继续使用现有 GPUI，不新增取消协议或修改依赖。
关闭 / 失焦仍清理会话，已有 dirty 内容仍须确认。详见 [D19](decisions.md#d19-关闭与失焦)。

## 1. 组件与来源

参考来源：`https://github.com/zed-industries/zed`。
冻结快照：`ba63a022c9a22ca66d68ef2a8f0bb9b5b0a9984d`。
本地阅读位置：`/home/leodt/ghq/github.com/zed-industries/zed`，不作为构建路径。

| 组件 / Zed 相对路径 | manifest 许可 | 使用方式 |
|---|---|---|
| `crates/sum_tree/` | Apache-2.0 | 选为 Rope 和显示索引的汇总树基础，提取核心实现与相关测试 |
| `crates/rope/` | GPL-3.0-or-later | 选为文本存储、文本摘要、UTF-8 / UTF-16 坐标基础 |
| `crates/gpui/src/text_system*` | Apache-2.0 | 优先调用当前 GPUI 对应 API，不引入另一套 GPUI |
| `crates/editor/src/display_map/wrap_map.rs` | GPL-3.0-or-later | 按自有快照裁剪增量 patch、后台换行、版本化结果等实现 |
| `crates/editor/src/element.rs`、`input.rs`、`scroll.rs` | GPL-3.0-or-later | 按需提取可视行、命中、输入几何和滚动锚点算法 |

不整体引入 Zed 的 `text::Buffer`、完整 Editor、MultiBuffer、language 或 project。
这些组件所需的协作历史与语言 / 项目模型不属于本项目最小编辑目标。

Rope / SumTree 的外围适配包括 workspace 依赖、测试支架、tracing 插桩及 Rope 使用的小型工具函数。
保留核心算法与测试；提取中发现依赖闭包显著扩大时先审查，不为原样编译而复制无关系统。
具体文件布局在 R1 确定，遵守单 crate 和少量职责模块原则，不提前建立通用后端抽象。

## 2. 源码导入与许可门禁

每批导入必须同时完成：

- 核对实际源文件与所属 crate 的许可，保留版权、许可证文本及适用 NOTICE。
- 记录上游仓库、commit、路径、本地路径、修改范围和保留的测试。
- 区分参考设计、复制实现与独立实现；改写代码不自动消除来源义务。
- 遵守 GPL-3.0-or-later 组件对整合作品分发的要求，不将 GPL 源码改标 MIT。
  Apache 组件保留其许可，自有代码的额外授权不覆盖第三方代码。
- 首次导入时补齐项目分发所需的许可与归属文件；未完成前不先在 Cargo 中声明统一许可。
- 后续按已验证问题选择性吸收上游修复；代码在本仓库维护，不要求另建 fork。

## 3. 最小正式纵向切片

```text
文件 / scratch → 自有文本模型 → 显示映射 / BufferView
                              → GPUI shaping / 绘制 / 输入 handler
```

R1 恢复正文显示、光标、最小输入 / 替换、滚动和换行。完整 keymap、历史分组与 minibuffer 分别在 M1 / M2 完成。
文件读取沿用现有文件层，不另起编解码实现。

### 状态与编辑

- 已提交正文、临时预编辑、显示快照分离；纯 marked 更新 / 丢弃不改变 modified，平台显式正文替换经 Edit 生效且不回滚。
- 提交经过唯一 Edit 入口，具有明确的替换范围、内容、来源、目标与版本；不以长度差猜测变更。
- UTF-8 字节、UTF-16 单元、逻辑行列、显示坐标与像素坐标显式转换。
  Emacs 字符运动不等同于 grapheme 运动，不能把显示边界直接当作命令语义。
- view 拥有 viewport / scroll / 布局，输入会话绑定目标与代次。目标失效后的事件不得按当前焦点重解释。
- R1 记录最小提交的 dirty 与关闭确认，完整历史分组、保存点和 marker 回放在 M1 验收。

### 显示与异步

- 昂贵 shaping / 绘制限制在需要的显示区域，换行索引可增量或后台补全。
- 有后台结果时区分临时与最终精确快照；以文本、宽度、字体、字号和其他布局参数识别版本。
- 过期任务不覆盖新状态，销毁的目标不再更新；输入和布局同时变化时也有回归。
- caret、命中、滚动定位与 IME 几何使用兼容版本的映射，未就绪布局不能产生错误坐标。
- 明确前台同步工作的范围、等待上限及超长单行路径；行数少不等于任务足够小。
- 参考 Zed 的前台 5 ms 等待机制不等于承诺 5 ms 完成全文换行；量化预算经测量确认。

### 输入与几何

- 覆盖中文、emoji、组合字符、tab、空行、EOF、行首尾、软换行边界、横纵滚动和非零文字原点。
- 平台输入 handler 注册的区域与实际绘制区域一致，使用 UTF-16 范围契约。
- 文本 / 预编辑更新到下一次布局完成之间，不能用新偏移读取不兼容的行缓存。
  同长度更新也须使相应内容与几何版本失效。
- 验证范围查询、反向字符命中、布局就绪后的平台坐标刷新，稳定帧不重复刷新。
- 普通组合最终提交只插入一次；取消仅移除仍 marked 文本，不恢复平台已经替换的正文。unmark 保留文字，失焦 / 关闭不强制提交。
- D17 组合期间拒绝编辑器命令 / 前缀；D19 关闭 / 失焦取消预编辑，已有 dirty 内容仍需确认。
- headless 与真实 Wayland / fcitx5 分开记录；真实环境测试先征求同意，不将自动化几何结果当作候选窗实机验收。

### 性能与完成条件

- 开启软换行测量确定性 8 MiB 混合、短 ASCII、短 Unicode、长 ASCII，以及单条超长行。
- 覆盖冷首帧、输入 / 删除、换行 / 合行、滚动、窗口改宽、字体 / 字号变化和后台任务完成。
- 分别报告首帧、最终精确布局就绪时间、前台更新与空帧；不靠截断正文或关闭换行通过。
- 固定样本、环境、构建模式及测量边界；headless 数据不表示 GPU 帧率。
- 首轮测量后提交量化预算并确认。明显前台全文阻塞或无法解释的回退阻止完成 R1，不留到 M3 再处理。
- 功能、性能和真实平台结果都有可复现记录后完成 R1；任何未验收项明确保持未完成。
