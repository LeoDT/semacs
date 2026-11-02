# 阶段 D：最小脚本宿主冒烟

## 身份与证据

- 用户在正常宿主终端运行 `fish scripts/acceptance.fish scripting .pi/tmp/d-scripting-host-1 --consent`，回传完整目录并确认主要功能正常，同时报告错误信息与 mode line 重叠。
- 基于 `9e904c2ec841304d9066b4778d18bb6f9315d369` 的未提交工作区；批次保存完整源码 patch（含未跟踪实现文件）。产物核对时与当前源码快照一致。
- 正式 release 二进制 SHA-256：`e6d6f9d18d3e48f668dc5878cab228d46e6a52f5b8b646946ab9bd8735feee56`，批次文件、报告与核对时的二进制一致。
- Arch Linux x86_64，Wayland / Hyprland，Rust 1.99.1，`LANG=en_US.UTF-8`；`fc-match monospace` 为 Noto Sans CJK SC Regular。没有字体实际命中或像素尺寸的独立测量。
- 使用隔离 `XDG_CONFIG_HOME` 和两份可丢弃文本；不截图、不注键、不代改输入法、不运行性能矩阵。

## 结果

`scripting/result.json` 的 `completed=true`，六项人工观察均为 yes：首次配置显示、键位与 `M-x` 变换及单次 undo、成功重载、失败重载保留旧配置、错误配置启动回退、回退后的原生编辑。两个进程正常退出，退出码均为 0，无失败清理；两窗 stdout / stderr 为空。

独立核对最终磁盘：

- `commands.txt`：`xalpha beta\n`。
- `fallback.txt`：`Zfallback\n`。
- 初始 / 重载 / 错误配置快照 SHA-256 与 fixture manifest 一致，最终入口文件保持本批故意抛错的配置。
- 报告中的中间字节检查全部通过；中间状态由驱动当时的报告支持，离线时直接核对的是最终文件及保留的配置快照。

## 显示缺陷与结论边界

用户另外明确报告：**错误信息与 mode line 重叠**。因此工具完成与六项 yes 仅支持脚本主要功能的确认，不表示错误信息布局或整个阶段无缺陷；该反馈不能被工具的 completed 标志覆盖。

初步代码检查发现 echo area 固定 24px，未限制多行文本溢出，而 JS 异常可包含换行堆栈；与现象相符，尚未单独原生复现。本轮无截图，不声称已目视定位具体重叠像素。修复与消息历史 / minibuffer 展示的范围须先确认，待办进入 `docs/plan.md`，不改写这批修复前证据。

“未观察到默认字号闪变”仅为用户观察，不是逐帧证明；性能、真实 IME、GPU / 实际呈现均未测。后续若需宿主定向复测，须另行取得该轮同意，不自动重跑本批。

## 后续收尾确认

用户随后明确确认 D 按本轮范围完成；错误显示缺陷保留，由独立 D1 任务处理，D1 之后再继续 E。此确认不改变上述修复前结果与未测边界；D1 的目标统一见实施计划。
