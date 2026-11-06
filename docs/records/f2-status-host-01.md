# F2 status 单窗口宿主功能验证 01

## 身份与环境

用户在本批获准的一窗口范围内运行 `magit-status`，回传 `.pi/tmp/f2-status-host-01/`；助手只离线核对，没有重启窗口或重放 Git 写入。本记录不以 `COMPLETED.txt` 或正常退出单独判定通过。

- 基础 HEAD：`6d620640b0b4a5361651563e867f360a510e3455` 加未提交 F2 实现；`source.patch` SHA-256：`4227f9b28f484fb6d6e06be73d77c68c19b6995429f181be67bcb4b70608f4ac`。在整理本记录前，重新生成的 HEAD 相对 tracked 差异及 untracked 源码与回传 patch 逐字节一致，HEAD / status 同样一致。
- release 主程序 SHA-256：`73f552b449a66e38ad33f29fc9ef378bab0a43d2db75323434af77610b321572`；构建成功，外层 `binary.sha256`、驱动结果及离线复核的实际二进制一致。
- 驱动、离线回归及 fish 入口散列与 [工具准备记录](f2-host-tools-local.md) 一致，包含默认用户 ignore / attributes 隔离修复。
- Linux `7.2.11-arch1-1` x86_64，Rust `1.99.1`，Git `2.56.0`，Hyprland / Wayland，`WAYLAND_DISPLAY=wayland-1`，`LANG=en_US.UTF-8`。
- 子进程使用 fixture 仓库为 cwd、明确文件参数及独立空 `XDG_CONFIG_HOME`；系统 / 全局 Git 配置禁用，无日常 Git 环境覆盖。fixture manifest SHA-256：`0c965dc71330eed14392bc1b38a6759f943f690442f13feee53eb44c28710725`。

用户尺寸 / 可读性备注原文为“没问题”，没有给出实际窗口尺寸；三组观察均确认 yes。空配置使用产品默认字体设置，但没有平台字体解析诊断；环境日志的 `fc-match monospace` 返回 Noto Sans CJK SC，不能据此推断 semacs 的实际默认字体解析结果。不补填尺寸或字体解析结论。

## 用户观察

三个分组均为 `yes`，视觉 / 焦点结论来自用户，不是助手独立目视复现：

| 分组 | 实际限定范围 |
|---|---|
| `status-display-focus` | 四组清单及数量、头部 / XY / 路径 / 当前 section / 光标可读性；组折叠 / 展开、叶节点 TAB no-op，双 view 共享折叠及独立 point / 焦点 |
| `status-stage-stale` | 在右侧 status 通过默认 `s` stage 一次，清单自动刷新且不抢焦点；左侧 diff 保留旧快照并显示 `[stale]`，status 本身已刷新 |
| `status-unstage-refresh` | 在 Staged 文件行通过默认 `u` unstage 一次；左侧 diff 仍过时，显式 `g` 后清除标识，双 view 焦点保持合理 |

只有一个 semacs 测试进程及一个窗口（含内部分屏），正常关闭返回 `0`，无强制清理、自动注键、IME 切换或截图。窗口 stdout / stderr 均为空，结果没有错误，批次没有 `STOP.txt`。

## 独立磁盘与检查点

不能只因最终 index 恢复到 base 就推断中间 stage 发生。驱动在用户执行 unstage 前已独立检查完整 index；离线核对该检查点的顺序、所有成功标志及散列与已知字节 oracle 一致，没有重跑写操作。

| 检查点 | 预期 index | 完整 index 条目 SHA-256 |
|---|---|---|
| 启动前 | base | `170d28ea2cfd211429230f34a4c831e6f012b6a7438b48173a85129be9f353c4` |
| 显示 / 焦点组后 | base | 同上 |
| stage 组后、unstage 前 | stage | `9411e0963109b38b2841596956be76e979f3f8b9b323f38ec6da262de1815cfd` |
| unstage / 刷新组后 | base | base 散列 |
| 关闭后 | base | base 散列 |

每个检查点均核对全部 index 的 path / blob OID / mode / stage，并确认 worktree、空配置、HEAD / refs 和 Git 配置不变；目标 stage 为 `WORKTREE current`，unstage 恢复其 `HEAD base`，无关 `10-staged.txt` 的 `INDEX other` 暂存内容保留。

离线再次只读检查最终完整 index、HEAD 与 index 各 blob 的已知字节、全部 worktree 路径 / 字节、未跟踪文件、配置、refs 及 fixture 身份，通过。没有把 index 缓存字节或新增对象要求为不变，也没有把视觉 yes 代替内容核对。本次审阅日志保留于 `.pi/tmp/f2-status-host-01-offline-review.log`，原回传目录保持不变。

## 结论与未测边界

**本批授权的单窗口功能范围通过离线核对**：用户确认的 status 可读性、默认 `s` / `u`、过时标识与双 view 焦点，以及独立的 stage / unstage 中间和最终磁盘结果一致。

它不是单独的完整阶段验收：特殊路径 / 支持矩阵、过滤器 / hook、并发、reload / kill / 另存、活动写入退出及不确定结果等仍依据已有隔离自动化证据，不外推成本批宿主实测；完整 F2 收尾还须核对长期契约、设计材料和遗留边界。实际窗口尺寸未提供；原生 CPU 时延 / draw / 滚动 / idle、真实 IME、GPU 执行及实际呈现时延均未测，不因此关闭历史性能观察或宣布总体性能预算通过。后续新增宿主批次须重新取得同意。
