# F6 tree viewer 单窗口宿主功能验证：host-01

本记录冻结 2026-11-13 用户获准运行的一窗口 F6 定向结果，不追随后续代码更新。执行入口与停止规则见 [F6 定向流程](../acceptance.md#f6-最小定向验证)，产品行为见 [F6 契约](../emacs-fidelity.md#阶段-f6文件夹-tree-viewer)，阶段摘要见 [F6 归档](../archive/f6-tree-viewer.md)。

## 批次与身份

- 用户回传目录：`/home/leodt/src/semacs/.pi/tmp/f6-host-01`；模式 `tree`。仅一个 semacs 窗口及其内部侧窗 / split，使用可丢弃 project、独立空 JS 配置和子进程专用 history state。不自动注键、不改变输入法、不截图、不测性能。
- 基础 HEAD：`46f77f41260f126bb0cbc384db9f8b63778945d6`。受测对象为累计未提交的 F6a / F6b / F6c 工作区及宿主工具，不能只用基础 HEAD 标识源码。
- `source.patch` 包括 tracked 差异与 untracked 源码，SHA-256：`2cc577eae3852e2daf67484f18dceb687982383fe94413384d0853c567a87117`。离线重新生成完整 tracked / untracked patch，与回传逐字节相同；Git status 与批次快照一致。随后仅更新收尾文档 / 本地清单。
- `cargo build --release --bin semacs --target-dir …/target` 成功。release 二进制 SHA-256：`e82107e86039aae6be76b84fc07222598c34a7986911425e893aaadaed7f6cbe`；回传摘要与离线读取的实际二进制一致。
- `files/fixture.json` SHA-256：`af6aa411a0503672c2aa4e16deb90fff6f1e107464289c24f77799202061fca2`。
- `files/instructions.txt` SHA-256：`42ca892d52f31b2ec94b82411a5fe57f7b2e4c532dbdda5dd218c7dcd915e389`，与受测 fixture 工具中的操作步骤一致。
- `files/result.json` SHA-256：`3b62431be9a8d251c90fd35849506150256745588625139bd6425c417095e830`。

## 环境与观察来源

Linux `7.2.14-arch1-1` x86_64，Hyprland / Wayland（`wayland-1`），rustc `1.99.1`，LANG 为 `en_US.UTF-8`。测试子进程使用批次下 `files/config` 与 `state`；空 `init.js` 保持不变，command / file-name history 留在隔离 state 目录。

环境中的 `fc-match monospace` / Bold / Italic 返回 Noto Sans CJK SC；这是系统 fontconfig 信息，不证明产品实际采用的字形或 fallback 路径。配置为空，未额外指定字体。`notes.txt` 为空，用户没有提供实际 / 大致窗口尺寸、字体诊断或额外说明；不从建议尺寸或程序默认值补齐宿主尺寸。

三组人工观察均为 `yes`：

1. **tree-side-window**：左侧布局、默认树内容可读、窗口切换、`C-x 1` 的侧窗保护与树中报错、关闭 / 恢复、三态 toggle 及宽度调整。
2. **tree-visits**：默认复用普通 pane、两种访问前缀的焦点与关闭行为、左右 / 上下 split、最近使用 pane 和访问后关闭树；目录 RET 开合，显式目录访问提示不支持 dired 且不创建 split。
3. **tree-locate-follow**：当前文件定位、隐藏树重新显示而不抢焦点、打开目录内文件后的 follow、关闭当前普通 pane 后跟随剩余文件、follow 关闭 / 开启及最终 Q 退出。

这些视觉 / 焦点结论来自用户依打印步骤给出的确认，助手没有独立操作或截图复现。报告为 `completed=true`、`errors=[]`；存在 `COMPLETED.txt`，没有 `STOP.txt`，应用 stdout / stderr 均为空。脚本只在应用正常退出后进入逐组确认，不能把正常退出单独解释为功能通过。

## 独立离线核对

助手未重开窗口、未重放按键、未修改或重新生成回传报告 / fixture。只读核对结果如下：

- 九个 project 文件（包含隐藏文件和 `.git` / `.jj` marker）及空配置共十份字节，与源码中的预置内容和 manifest 摘要逐项一致。
- 完整 project / 配置路径与预期一致，没有缺失、多余路径或符号链接；原始操作说明未变。
- `observations.tsv` 的三组 yes 与报告一致，报告的路径 / 字节检查和离线计算一致。
- `.git` / `.jj` 仅是根发现 / 隐藏用目录 marker，不是真实 Git 仓库；本批没有运行 Git 写操作，也不提供 Git 集成证据。

离线核对摘要留在 `.pi/tmp/f6-host-01-audit.json`；原始批次保持不变。

## 本地证据与审查

与本批产品源码对应的 F6c 最终本地检查为 `cargo test --all-targets`（lib 717 项通过）、`cargo clippy --all-targets -- -D warnings`、fmt / diff 检查及 JS 示例 / 类型声明检查。宿主工具随后完成 102 项离线脚本回归及 fish 语法检查；宿主工具准备未修改产品 Rust 源码。原始本地日志为 `.pi/tmp/f6c-all-final.log`、`.pi/tmp/f6c-clippy-final.log`、`.pi/tmp/f6-host-tools-03.log`。

F6b / F6c 各进行一次集中审查。F6b 的排队操作意图、折叠 EOF 和 dedicated 拒绝打开生命周期问题，以及 F6c 的 minibuffer 吞 follow / 关闭普通 pane 未调度 follow 问题，均由主代理修复并补回归；审查后未追加 reviewer。本地回归覆盖异步目标隔离、快速组合键顺序、导航 / 定位、访问完成动作与 follow 的测试时钟行为，不冒充真实宿主观感。

## 结论与边界

本批批准的单窗口功能观察与独立路径 / 字节核对通过，结合本地自动化证据，完成 F6 首版已确认范围；没有发现阻塞性遗留，不自动追加宿主矩阵或进入下一阶段。

本批 follow 只确认空闲后定位、开关及焦点，**没有量化精确 0.2 秒**；宽度调整是用户观察，没有独立的像素 / 列几何诊断。窗口尺寸未提供。原生 CPU 性能、真实 IME、GPU 执行 / 屏幕呈现时延均未测，不宣称总体性能预算通过，也不改变历史已接受的超限观察。

根识别分支、懒加载 / 合并、刷新与节点删除回退、异步失败 / kill / 另存解绑等主要依据本地测试，未扩展成完整宿主文件系统矩阵。未覆盖的平台或首版排除的 filewatch、git 注解、dired / 文件管理、多根 / workspaces、远程等仍遵循有效契约，不因本批完成视为已实现。
