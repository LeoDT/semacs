# F1：只读 Magit diff 宿主定向验收

## 批次身份与证据

用户授权一个窗口、隔离配置与临时仓库的最小定向验证后，在正常 fish / Wayland 宿主运行 `magit` profile，回传 `.pi/tmp/f1-magit-host-01` 并确认“看着都没问题”。本记录是用户人工观察与助手离线核对，不是助手独立 GUI 复现。

- Git 基线：`d6a7ba4890f30e24b70794d78294535d9239a440` 加当时未提交 F1 改动。
- `source.patch`：239992 字节，SHA-256 `a199232bcd4af6e14848aa76c2a8e6c6b9f0cf0857bc2cceeacdf7e0b4c694ac`。归档文档更新前重新合成 tracked / untracked 源码差异，与回传 patch 完全一致；HEAD / git-status 同样匹配。
- release 二进制 SHA-256：`a9e9fb95164536be89ac1965b3354312bb9d270569c21a24afb7a7db88d49c1b`，与当前 `target/release/semacs` 一致；构建成功。
- 环境记录：Arch Linux x86_64、内核 7.2.11-arch1-1，Rust 1.99.1、Git 2.56.0，Wayland / Hyprland，`LANG=en_US.UTF-8`。
- fixture 使用一个窗口及其内部分屏、独立 `XDG_CONFIG_HOME`；测试子进程清除继承的 Git 环境覆盖并禁用全局 / 系统 Git 配置。配置字号为 20px，继承应用默认随包字体。`fc-match monospace` 的系统查询结果不是实际正文渲染字体证明。
- `notes.txt` 为空，未记录实际窗口尺寸；没有据建议的约 1000×600 推定实际尺寸。

原始目录保留 `source.patch`、`binary.sha256`、环境 / 构建 / 应用日志、fixture、人工回答与验证结果。助手只读核对，没有重跑桌面、修复 fixture 或覆盖原始结果。

## 人工观察

`observations.tsv` 与 `result.json` 四组均为 `yes`，用户另明确确认无问题：

| 观察组 | 确认范围 |
|---|---|
| `magit-display-navigation` | staged / unstaged 差异的可读样式、heading 与当前 section 反馈、diffstat 跳转、折叠指示、section / 父节点导航及翻页 |
| `magit-shared-refresh` | 同一 buffer 的左右分屏共享折叠、刷新保留折叠、各 view point 独立及合并 pane |
| `magit-horizontal` | 超过同步 shaping 阈值的长行不软换行，行末 / 行首的水平跟随与手动水平查看，光标 / 文字对齐可用 |
| `magit-version-visits` | staged 的旧侧 HEAD 与新侧 index 快照正确且只读，unstaged 新侧复用普通 worktree buffer |

详细手动步骤由该批源码中的 `scripts/magit_fixture.py` 固定，不在此重复维护键序列。

## 独立核对

- `completed=true`、`profile=magit`、`errors=[]`，有 `COMPLETED.txt`、无 `STOP.txt`。
- 应用 stdout / stderr 均为空；没有记录异常退出。
- 两份 worktree 正文及 `init.js` 与代码内的确定预期字节一致，共 3 项通过。
- HEAD 身份、index 原始字节、Git 配置原始字节、tracked 路径集合，以及两份文件在 HEAD / index 中的 blob 内容，共 8 项通过；助手再次以固定 fixture git-dir 只读核对，结果一致。
- HEAD / index / worktree 预期分别具有 `HEAD-OLD` / `INDEX-NEW` / `WORKTREE-NEW` 身份，避免把正确的文本外观误当作正确版本访问。

## 结论与未测边界

结合 [F1 本地验证](f1-magit-diff-local.md)，本批已批准的四组宿主功能通过，用户确认无问题；F1 可按本轮只读首版范围完成。阶段状态与后续安排见 [实施计划](../plan.md)。

没有截图、自动注键、脚本输入法变动或原生性能测量；实际窗口尺寸未提供。人工确认不外推为 IME 时延、GPU / 实际呈现测量、所有平台键位、超大 diff 容量、原生持续输入或完整性能矩阵通过。E3 已接受暂缓项与原有性能预算保持不变，不因本次功能验收自动补测。
