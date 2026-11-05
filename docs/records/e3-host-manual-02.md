# E3 随包 Lilex 宿主人工复核：四组通过

## 身份与范围

用户回传 `.pi/tmp/e3-lilex-manual-20261105-100955/`，确认整体没有问题，另报告中文输入时的轻微视觉跳动并明确本轮可不解决。本批为 [随包 Lilex 本地修复](e3-bundled-font-local.md) 后的新目录人工复核；[首批失败](e3-host-manual-01.md) 保持原样，不改写为通过。

- HEAD：`138995b31a615833e04a729e8d619c829b9667c4`，含未提交 E3 实现及字体修复。
- 登记本记录前，回传 `source.patch` 与当前 tracked + untracked 差异逐字节一致；patch SHA-256 为 `ea9c0d8535c762667e4d51b72e05430b204209d26b41d75712f47dbd8219dbf9`。
- 二进制 SHA-256：`dfef08908cbe029d95481d5bd559208cfa9c5af8671f7f6bc549e3256b29f14f`，与核对时 `target/release/semacs` 一致。
- release 构建成功，stdout / stderr 均为空，无 STOP；`result.json` 为 completed=true、errors=[]。正常退出与 COMPLETED 标记本身不替代人工观察或字节核对。
- 环境：Arch Linux x86_64、Hyprland / Wayland、rustc 1.99.1。未提供准确窗口尺寸与输入法名称。

## 人工观察

`observations.tsv` 四组均为 yes，用户进一步确认“没什么问题”：

| 组 | 本批确认范围 |
|---|---|
| styles-fonts | Rust 粗体 / 斜体、下划线 / 删除线及混合文字显示 |
| styles-layout-theme | 选区 / 光标、分屏、主题取消 / 恢复强调、窄窗口和字号变化后的布局与点击 |
| styles-rust-ime | Rust 正文真实预编辑 / 取消 / 提交、undo / redo、保存 |
| styles-markdown-ime-save | Markdown 标题 / 强调 / 粗斜体及围栏、主题、正文 IME 与保存 |

这是人工确认，不包含运行时 glyph / FontId trace；字体实际命中的无系统字体确定性证据仍见本地修复记录。

## 独立字节核对

读取原始 fixture manifest、正文和配置，在不调用会写回 result.json 的 verify 函数下，独立核对当前生成器的 seed 身份与预期最终字节：

| 文件 | 最终 SHA-256 |
|---|---|
| `body.rs` | `ae4f2d441ca0e9ae8702f85b55e10e9882475c6eeec1cf66c9f30646e3b1028b` |
| `body.md` | `7bfa811d0d120eb36b78357da9a5ef802610c3819bc5db4183edb9524643fbcc` |
| `config/semacs/init.js` | `52b3b51c0c144477a5128e82458bc0bdee1a4e5c2f634dd7a3047055c4e71aa3` |

两份正文仅在各自首行冒号后增加恰好一个“你”，其余字节与 seed 一致。配置保持 seed 原样，使用 `semacs.setFont({size: 20})`，没有覆盖随包默认字体族；rich / plain 的样式声明未变。四组观察、配置和最终正文均与 result.json 一致。

## 用户接受的非阻塞观察

用户描述：输入中文时感觉该行行高有变化，同行英文会稍向下跳约 1–2 px；并明确“这个可以不解决”。原始 notes.txt 为空，该补充来自用户回传消息。

本轮没有仪器测量，也未区分实际行高变化与字体基线 / ascent 导致的视觉位移；不推断根因或将其他编辑器也有此现象当作正确性证据。按用户意见登记为非阻塞，不改代码、不追加诊断或复测。后续若实际影响使用，再确认范围处理。

## 结论与未测边界

E3 本批人工字体 / 装饰、主题 / 布局和两份正文 IME / 保存复核通过；首批粗斜体不可见阻塞在此人工范围内解除。

原生性能仍未运行，IME 时延与 GPU / 实际呈现测量均未测，没有截图。中文视觉跳动未被本批自动数据量化；整行背景原语继续由本地几何回归证明，不外推为宿主整行背景验收。没有因人工通过调整性能预算或宣布 E3 全阶段收尾。

下一步为既定 `styles-perf` 代表性原生 CPU 批次，须另行获得用户同意：mixed 约 8 MiB、Markdown 约 256 KiB，各三轮、最多六个顺序窗口。当前只核对回传文件和登记结果，没有启动新窗口、探针或测试。不进入 F。
