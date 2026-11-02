# 阶段 B：候选式 minibuffer 宿主人工验收

本记录冻结用户于 2026-11-02 执行的定向人工验收与助手离线核对结果。行为正文见 [候选式读入](../emacs-fidelity.md#候选式读入阶段-b)，先前的 headless 证据见 [本地验证](b-minibuffer-local.md)。

## 结论

候选交互、文件 / buffer 键盘闭环、布局 / 字号与正文 / minibuffer IME 的六组人工观察全部确认为通过；独立复核的 18 个 fixture 文件字节全部符合预期。结合本地回归，阶段 B 在已确认范围内验收完成。

本轮不运行原生性能矩阵、不自动注键，也不据此声称原生性能预算、所有字体或所有窗口尺寸均已验证。视觉与真实 IME 结论来自用户观察，不是助手独立重现或自动像素断言。

## 产物与身份

- 输出目录：`.pi/tmp/b-candidates-20261102-094906/`；模式为 `candidates`。存在完成标记，无 STOP 文件；正式窗口正常退出，stdout / stderr 均为空。
- 基础提交：`05d4a4e213b286306b5655a8e3a5aaa226ef472c`，加该批次保存的未提交工作树。离线核对时，保存的 `source.patch` 和 `git-status.txt` 与当前工作树重新生成的结果逐哈希一致，包含未忽略的新文件。
- `source.patch` SHA-256：`e21fbc0799b11ce7039ebedf81584640cfadf4ef816dff6a5364ba37ab0c6496`。
- 实际 release 二进制 SHA-256：`585616adf677da328dc42fe2db5f5b03e47504a2e34d0afd0ba252c6bbb678ad`；保存值与磁盘二进制核对一致。
- 与本地记录相同算法的 `src` / `tests` / `examples` / Cargo 文件集合 SHA-256：`35d589b6fd37b20ea7ae350d3a3f0647064a5846ebd9d58a133896ee23c6f57b`。这已包含用户批准的 mark 默认键位调整；归档文档不参与此身份。
- 构建日志确认 `cargo build --release --bin semacs` 成功；没有为此轮启用 profiler。

原始目录保留构建日志、源码差异、二进制身份、环境、fixture、观察、字节检查及说明。此处提炼关键证据，不将临时目录作为长期结论的唯一存放处。

## 环境与观察边界

- Arch Linux x86_64，内核 `7.2.11-arch1-1`；Wayland（`wayland-1`）、Hyprland。
- Rust：`1.99.1 (58cf8f9c3 2026-10-13)`；locale：`en_US.UTF-8`。
- `fc-match monospace` 与 `fc-match sans-serif` 均报告 `NotoSansCJK-Regular.ttc` / `Noto Sans CJK SC` / `Regular`。这不是 GPUI 实际字体命中的证明，也不证明两个字体族实际使用了不同字体。
- 用户确认了正常 / 窄窗口、长名称 / 注释、输入行标识与 20px 字号流程。补充说明仅为 `OK`，未记录精确窗口尺寸、截图、实际字形字体或 IME 引擎版本，故不追加精确像素、具体字体覆盖或平台矩阵结论。
- 未记录 GPU / 驱动身份、渲染器分类及 CPU / GPU 时延；不将存在 `/dev/dri` 的入口检查当作硬件渲染或性能验收证据。

## 人工观察与独立字节核对

`observations.tsv` 与 `result.json` 一致，下列六组均为 `yes`：

| 组 | 用户确认的范围 |
|---|---|
| candidate-command | 无输入候选、评分命中高亮、导航、TAB / RET / M-RET、prompt 保护、mark / point 与焦点恢复 |
| candidate-files | 目录进入 / 输入行默认、新名称接受、文件保存与覆盖安全确认 |
| candidate-buffers | 默认项、路径 / dirty 注释、关闭确认、原始名称必须存在 |
| candidate-layout-font | 字体设置 / 字号、正常与窄窗口、候选可见性、长名称 / 注释截断 |
| candidate-minibuffer-ime | 预编辑冻结、输入法候选操作、提交 / 取消及正文隔离 |
| candidate-body-ime | 正文提交、取消、不重复输入与保存闭环 |

助手只读复核了 fixture 的 profile、16 个 seed 哈希、六组观察及全部 18 个最终文件；没有重新运行窗口或覆盖原报告：

- `source.txt` 保持 `alpha second\nnext line\n`；`alpha-existing.txt` 保持 `outside\n`。
- 新文件 `alpha` 精确为 `NEW-A!`，新文件 `al` 精确为 `NEW-B`。
- `overwrite.txt` 精确为 UTF-8 `你Xalpha second\nnext line\n`。
- `sub/child-document.txt` 与 12 个长名称文件均保持初始字节。
- 没有意外新增的 fixture 文件，结果中无错误。

## 本地回归与未测项

用户批准 mark 键位调整后，347 项 Rust 测试、6 项离线工具测试、默认及 `native-profiler` feature 的 Clippy、格式与差异检查通过；正文和活动 minibuffer 的 `C-t`、备用 `C-@` 及移除 `C-SPC` 绑定有定向覆盖。冻结的 Emacs 参照未改写。

先前本地记录的 345 项测试及 headless 数值保持其原源码身份，不追写为 mark 调整后的性能复测。本轮没有新增性能计时、平台 OS 关闭验收或其他宿主矩阵；这些未测边界不被人工功能通过覆盖。
