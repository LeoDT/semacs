# E1 Rust 语法：宿主定向验收

## 批次与身份

- 用户已同意本轮真实环境验证，在正常宿主终端运行 `fish scripts/acceptance.fish syntax .pi/tmp/e1-syntax-host-01 --consent`，并回传整个输出目录。助手只离线核对产物，没有重跑窗口。
- 源码：`c92cc320825050c102730aec62b21dfad2149f95` 加批次保存的 `source.patch`，SHA-256 为 `6c1da54eb6d944d8782341881b5aba74af655718116886982fde4cac4141d4bd`。离线核对时该 patch 与当前完整跟踪 / 非忽略新增源码差异逐字节一致；E1 代码身份另见 [本地验证](e1-syntax-local.md)。
- 宿主只构建正式 release `semacs`；二进制 SHA-256 为 `076ef1edc74e97bb5a48e9ae3cf0faafb606f03ca41ecd0a82720c1fcee0e9f5`，与回传清单及离线实际二进制一致。此处记录宿主独立构建身份，不覆盖本地多目标构建的旧二进制身份。
- 环境：Linux x86_64 `7.2.11-arch1-1`、Rust `1.99.1`，Wayland / Hyprland，`LANG=en_US.UTF-8`；`fc-match monospace` 返回 Noto Sans CJK SC Regular。这不是逐字形实际字体命中的证明。
- 独立 `XDG_CONFIG_HOME` 和可丢弃文件；不截图、不注键、不改变输入法。源码 / 构建 / 环境、stdout / stderr、fixture、五项观察、字节结果与完成标记均保留。

## 观察与独立核对

以下五项均由用户回答 `yes`，属于用户真实窗口观察，不是助手独立目视复现：

| 观察项 | 本批确认 |
|---|---|
| `syntax-start-split` | Rust 分类、字符串内关键字、共享分屏、region 及方块光标可读 |
| `syntax-theme` | light / dark / 自定义语法配色切换，关键字用户覆盖保留，两个 pane 更新 |
| `syntax-edit-history` | 未闭合块注释、undo / redo 的正文与高亮恢复 |
| `syntax-language` | 相同内容的普通文本无语法色，另存 `.txt` 后停用、`.rs` 后恢复 |
| `syntax-layout-save` | 窄窗口 / 字号、读入配色及正文保存恢复 |

助手按当前 fixture 定义独立重算并核对，而非仅接受 `result.json` 的布尔值：

- 原始 `body.rs`、`plain.txt`、隔离配置及 manifest 的种子哈希均一致。
- 新 `language.txt` 为原文；`restored.rs` 精确为 `// done\n` 加原文。
- 五项观察均为 yes，`errors` 为空，无 STOP；stdout / stderr 均为空，入口正常完成。
- 额外 `restored.ts` 为未修改的原文。用户确认这是另存时将名称误输为 `.ts`、随后改为 `.rs` 所致；它不是误判 TypeScript 为 Rust 的证据，不影响预期文件与已确认的语言切换结果。原文件保留，不修复或清理 fixture。

尺寸备注仅填 `yes`，没有提供正常 / 窄窗口的具体尺寸；按未记录保留，不据此追加补测。没有截图，颜色与光标可读性以用户本批回答为证。

## 结论与边界

E1 已批准范围的五项宿主定向功能观察和独立磁盘 / 配置字节核对通过；额外文件已澄清，可按本轮范围收尾。与本地验证共同覆盖本阶段实现，不扩大为全部尺寸、字体、原生时延或 GPU 验收。

原生性能、真实 IME、GPU 执行 / 屏幕呈现仍未测。旧 M2 profiler 生命周期自检失败不由本批解决；按用户顺序在 E1 后单独修正 / 验证工具，不回写旧失败记录或自动运行原生性能矩阵。
