# R1 原生输入验证记录

## 范围与结论

用户明确授权后，首轮于 2026-10-31 在真实 Hyprland / Wayland / fcitx5 会话执行。
首轮普通输入、Pinyin 预编辑 / 候选窗、提交 / 取消、软换行与滚动后的候选位置、
失焦 / 重聚焦、D17 屏障和关闭确认的已列场景通过。

**首轮使用软件渲染，不是硬件 GPU 性能验收，也不代表 R1 全部完成。**
后续用户在宿主终端完成 36 次非软件 RADV 路径的原生 CPU / 平台提交流程测量，已核对完整产物。
用户已批准原生预算，确认以已有真实功能证据 + 硬件 CPU / 提交流程测量完成 R1；GPU 执行 / 实际呈现和纯实体输入端到端延迟保留未测限制。其他输入法、X11 和完整 Emacs 命令链仍未验证，不能因 R1 完成而改记通过。
软件自检与宿主硬件结果分开记载于下文及 [性能记录](performance.md)。

## 环境和驱动方式

- Hyprland 0.56.2，Wayland；fcitx5 5.1.23、fcitx5-chinese-addons 5.1.15，Pinyin。
- Rust 1.99.1、Linux 7.2.11-arch1-1；其余机器环境见 [性能记录](performance.md)。
- 执行工具环境没有 `/dev/dri`。未强制软件渲染的有界启动探测出现 Mesa / Zink 设备初始化错误，
  8 秒后结束；这不是可用硬件环境的产品启动耗时。
- 后续设置 `LIBGL_ALWAYS_SOFTWARE=1`，窗口成功映射到真实 Wayland 桌面，
  使用原生字体、实际合成器及 fcitx5 协议路径。GPUI gpu_specs 报告 is_software_emulated=true，
  设备 llvmpipe (LLVM 23.1.3, 256 bits)，Mesa 26.2.5-arch1.1。未修改驱动、GPUI 依赖或桌面配置。
- 合成器报告的测试窗口位于非零桌面原点，本次主要为 (1723, 67)，854 × 1370。
- `wtype` 经合成器虚拟键盘注入按键，不是直接调用编辑模型或伪造 GPUI IME 回调；
  但不替代实体键盘、AltGr、死键及所有键盘布局的完整验收。
- `examples/native_input.rs` 仅观察生产 AppShell / BufferView，记录 committed / display、
  point、marked、modified 和 echo；输入、布局和关闭逻辑均走正式模块。
  观察日志会写入测试正文，只用于小型可丢弃样本。其时间戳和通知瞬间的 ready 字段不是性能测量。

## 用户提供的宿主硬件探针（非性能结果）

软件渲染验证之后，用户在正常桌面环境运行探针并反馈：

```text
is_software_emulated: false
device_name: AMD Ryzen 7 7700 8-Core Processor (RADV RAPHAEL_MENDOCINO)
driver_name: radv
driver_info: Mesa 26.2.5-arch1.1
```

这是用户提供的探针输出，确认宿主环境具有可用的硬件渲染路径。
当时尚未在此硬件路径上完成上述功能矩阵、原生性能测量或预算确认；
不能把宿主探针反馈改记成前述软件渲染测试使用了硬件 GPU。
后续硬件测量见下文；代理工具环境仍未获得 GPU 设备访问权限。

当时用户要求先交接，未实施新测量。续接后，用户已另行授权本轮独立性能窗口、测试按键及 fcitx5 操作；
现在已新增性能入口、完成软件渲染自检，并由用户在正常宿主终端完成硬件测量；工具仍无 /dev/dri。

## 实际结果

| 场景 | 结果 / 断言 |
|---|---|
| 原生显示 | 中文、emoji、组合字符、tab、空行、EOF 可显示 |
| 普通输入 / 换行 | 输入 ASCII，Return 新增 LF；观察到准确正文和 point |
| 删除 | Delete 删除正文首字符；Backspace 依次删除 combining acute、e、emoji，UTF-8 未损坏 |
| 普通预编辑 | 输入 ni，仅 display 含 marked；clean 用例的 committed / point / modified 不变 |
| 候选窗 | 截图观察到 fcitx5 候选窗在 caret 附近；非零窗口原点没有错误偏移 |
| 换行 / 软换行 | caret 移到新行、120 个数字形成软换行后，候选窗跟随到对应行 |
| 滚动后组合 | 连续 60 次 Return 触发 caret reveal / 视口滚动，再组合时候选窗仍锚定可见 caret |
| D17 | 组合中 C-x 不修改正文，不启动前缀，echo 显示 composition-active 提示 |
| 取消 / 提交 | Escape 清除 ni，已提交正文不变；再次 ni + Space 提交你，未重复插入 |
| 失焦 / 重聚焦 | hao 预编辑期间切走窗口，再回来无残留预编辑；已有提交保留，随后 Z 正常插入 |
| dirty + 预编辑关闭 | 已有提交且 ni 仍 marked 时发关闭请求；只丢弃 marked，显示 Cancel / Discard，原提交保留，确认前窗口不关闭 |
| 取消确认 | Escape、C-g、默认 Enter 均取消并恢复输入，随后可继续输入；dirty 保持 |
| 明确丢弃 | Tab 选择 Discard 后 Enter，测试窗口关闭，进程正常退出 0 |
| 只有预编辑时关闭 | 没有已提交修改，组合期间关闭直接退出 0，不产生 dirty 确认，不提交临时文字 |
| 清理 | 测试窗口 / 进程已清理；原测试文件 SHA-256 未变，原焦点和 fcitx 活动状态已恢复 |

候选窗位置是截图的实际观察，不是毫秒延迟、像素误差统计或所有缩放 / 字体 / 布局配置的证明。

## 本轮发现并修复的问题

原先 `Window::prompt` 在 Linux 回退到 GPUI 的最小鼠标确认框，没有键盘处理；
headless 的平台应答模拟跳过了真实确认框，所以此前测试未发现 Escape 无效。

现在统一通过 `semacs::app::init` 初始化应用，安装 `src/prompt.rs` 的外围确认视图：

- 使用 GPUI PromptHandle 保持应答和焦点生命周期，gpui-kit 按钮负责外围外观。
- 安全默认项为 Cancel；Escape / C-g 取消，Tab / Shift-Tab 或左右键选择，Enter / Space 确认。
- 不改变正文 keymap，不引入 CUA，也不以 Enter 默认丢弃内容。
- headless 关闭回归改为绘制真实确认框并派发按键，不再以平台应答模拟代替该链路。
- 修复后重新执行真实窗口取消 / 继续输入 / 明确丢弃；补充安全默认与 C-g 回归。

## 可复现入口

仅在获得桌面测试同意后执行，使用可丢弃输入，勿将真实文档作为诊断样本：

```sh
cargo build --example native_input
# INPUT_FILE 必须已存在；NEW_STATE_LOG 必须尚不存在，入口拒绝覆盖日志。
target/debug/examples/native_input INPUT_FILE NEW_STATE_LOG
```

在本次工具环境中，连接已有 Wayland socket，并为该进程设置软件渲染：
`WAYLAND_DISPLAY=wayland-1 LIBGL_ALWAYS_SOFTWARE=1`。
这只是当前环境的测试启动方式，不修改产品默认渲染配置，不代表推荐在正常硬件环境强制软件渲染。

按上表顺序在测试窗口操作，切换输入法只影响当前测试上下文，结束后恢复原状态。
状态日志以 TSV 保存，committed / display / message 为 UTF-8 十六进制；可据此独立检查文本，而非只看截图。
首个与末尾状态均应核对，关闭后另查窗口消失及原文件字节不变。

## 新增原生性能入口自检

续接批次显式使用 llvmpipe，六类约 8 MiB 正文各三次，共普通矩阵 18 次、
真实 fcitx5 矩阵 18 次；全部完整执行、样本未变、原焦点 / 输入法状态已恢复，测试窗口已关闭。
Pinyin 在初始位置及滚动 / 改宽 / 改字体之后验证预编辑、取消和提交；
候选截图保留作位置观察，不是呈现时间或 GPU 执行时间。

普通矩阵发现远滚动后改宽 / 字体的第一份场景没有正文，须等待后台全文完成。
已改为同一 display 身份下的旧可见行仅用于临时绘制，并按新区域裁剪；
输入几何不读取旧布局，新文本 / marked 更新立即丢弃保留快照。
修复后的全部矩阵无空白正文帧；headless 回归覆盖旧绘制仍可见而平台命中不可用、
新输入清除旧快照及后台最终布局恢复。

可复用入口、硬件运行命令和 CPU / 外部驱动计时边界见 [性能记录](performance.md#8-原生-cpu-绘制--平台提交测量入口)。
程序默认拒绝软件渲染；本轮仅以显式 `--software` 自检。
该软件批次不包含硬件性能、GPU timestamp 或实际呈现反馈，不据此标记 R1 完成；后续用户硬件结果单列如下。

## 用户执行的宿主硬件测量

用户运行正式性能驱动后，已核对普通 18 份 / 1278 行、真实 Pinyin 18 份 / 162 行报告。
全部 GPUI 设备记录均为非软件 RADV / Mesa 26.2.5-arch1.1，release 构建、相同样本与字体请求。
两组完成 / 恢复标志为 true，样本未变、stderr 为空、所有阶段无空白正文帧；
原始 CSV 重算汇总与 summary.json 一致。

真实 IME 仍经合成器 / fcitx5，覆盖初始及滚动 / 改宽 / 改字体后的预编辑、取消 / 提交；
抽查候选截图位于 caret 附近。硬件的 D17、失焦、关闭确认未在这两个性能流程中重跑，
它们的既有真实平台功能记录仍是前述软件渲染批次。

完整数据摘要与已确认预算见 [性能记录第 9 节](performance.md#9-宿主硬件路径基线与已确认预算)。
用户已明确要求标记 R1 完成：原生预算通过，D17 / 失焦 / 关闭沿用既有真实 Wayland 软件渲染证据，不要求仅因渲染器切换完整重跑。
GPU 执行时间、实际呈现与纯实体输入端到端延迟仍未测，作为已接受的 R1 限制保留。持续输入和完整命令链性能转入 M1；其他平台 / 输入配置按后续支持范围验证。

## 本地证据

临时产物保存在 `.pi/tmp/`，不作为正式测试唯一实现：

- `r1-native-hardware/`、`r1-native-ime-hardware/`：用户宿主运行的各 18 份硬件报告、环境 / 汇总、日志与 IME 截图，和软件自检分开保存。
- `r1-native-perf-retained/`、`r1-native-perf-ime-retained/`：本轮修复后的各 18 份 CSV、环境 / 汇总、进程日志及 IME 组合截图。
- `r1-native-perf-software-final/`：修复前远滚动重排空白的完整矩阵证据，保留与新结果区分。
- `r1-final-headless-perf-after-retained.csv`：修复后的 18 次 headless 回归；不计原生验收。

- `r1-native-report.json`、`r1-native-state.tsv`：完整编辑 / IME / 关闭流程，报告 completed=true。
- `r1-native-clean-report.json`、`r1-native-clean-state.tsv`：clean 取消及仅预编辑关闭，报告 completed=true。
- `r1-native-preedit.png`、`r1-native-preedit-wrapped.png`、`r1-native-preedit-scrolled.png`：候选位置。
- `r1-native-dirty-close.png`、`r1-native-close-escape.png`：修复后的确认和取消。
- `r1-native-before-prompt-fix.json` / `.png` / `.tsv`：修复前 Escape 无效的证据。
- `r1-native-input.py`、`r1-native-clean.py`：本轮环境的自动化脚本，发送按键前检查当前焦点是测试窗口。

正式可复用入口为 `examples/native_input.rs`；正式 headless 回归在 `src/view/input_tests.rs`。
临时证据丢失后，可用入口与上表重新生成，不依赖临时脚本才能验证。
