# F2 单窗口宿主工具本地验证

## 身份与范围

基础提交 `6d62064` 加含 F2 累计实现的未提交工作区。本记录只冻结宿主入口准备及其离线验证，不是宿主运行或通过记录。

- 驱动 `scripts/magit_status_manual.py` SHA-256：`bb89003b80cb7a2e4f9ff13ab0a6910b0f01ffb973acbe8640b496764aaa26bb`。
- 离线回归 `scripts/test_magit_status_manual.py` SHA-256：`51dab1d7b544b3d92b20bca19ebba79e1223851c3f70f992bce4fc2797c97ed1`。
- fish 入口 `scripts/acceptance.fish` SHA-256：`e1969646ce2e22ab2a78f870796f2f20f5f9ceb105acbdca4082c7daaff475f9`。

用户已批准一窗口及内部分屏的 status 可读性、默认 `s` / `u` 和焦点观察；不自动注键、不切换 IME、不截图、不测性能。实际操作和资源保护见 [F2 定向流程](../acceptance/profiles.md#f2-最小定向验证)，键序列只在驱动维护。驱动及本地测试未启动桌面，实际 fish 批次尚待用户运行。

## 本地结果

- `scripts/` 离线 81 条通过，含新增 17 条 fixture / mock-child 回归；`tests/` 离线 110 条通过。
- 新工具用隔离 Git fixture 验证 base → stage → base 的完整 index oracle；不写入或提前恢复时不能通过 stage 检查点，无关 index 条目变化同样拒绝。人工 yes 不替代磁盘检查。
- mock-child 覆盖单次启动、每组停点、no / EOF、窗口提前退出、字节 / 二进制 / 配置变化、已有或链接输出拒绝、独立配置 / Git 环境、正常关闭等待和超时强制清理标识；没有运行真实 semacs 窗口。
- 集中审查发现 P2：禁用全局 Git 配置后仍可能读取默认用户 Git ignore / attributes。已在 fixture 显式设置 `core.excludesFile` / `core.attributesFile` 到 `/dev/null`；以合成 HOME / XDG 默认 `*.txt` ignore 及 UTF-16 attributes 覆盖初始化和 stage 回归，未修改用户真实配置。由主代理定向及全量离线复核通过，无待修复审查项。
- `fish --no-execute scripts/acceptance.fish`、`cargo build --release --bin semacs`、格式 / diff 检查通过。没有产品 Rust 改动，不据工具变动机械重跑 Rust 全量；此前产品 / headless 结果见 [F2 本地记录](f2-status-local.md)。fish 的源码归档改为 HEAD 相对，包含 staged、worktree 与 untracked 源码，不只归档未暂存差异。

本地日志保留于 `.pi/tmp/f2-host-tools-review-fix-01.log`、`f2-host-tools-script-tests-final.log`、`f2-host-tools-offline-tests-final.log`、`f2-host-tools-release-build-01.log`；审查前通过结果也保留，不覆盖。正式宿主批次会在新输出目录重新构建并捕获自身的源码 / 二进制身份、环境、fixture、各检查点和人工回答。

结论：最小宿主工具已就绪；尚无该批真实键盘 / 焦点、字体可读性或其他宿主通过证据，原生性能、IME、GPU / 实际呈现仍未测。失败须保留整个目录，不修复 fixture 或自动重试。
