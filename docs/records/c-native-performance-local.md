# 阶段 C：多 view 原生性能工具本地验证

## 身份与环境

- 未提交工作树，基于 `d7b0209775855785dbfe8acfc535deb2e78784e5`；本次修改测量工具及文档，不修改产品运行时。
- Rust / Python / fish / Cargo 源码身份汇总：`88ca0ff6c96e74e59c064d7856cc5b524571557d83e18e2501f7e8dfecef2466`。包含新增工具 / 测试，不包含文档、临时产物或用户原有的 `.pi/settings.json` 删除：

  ```sh
  { find src tests examples -type f -name '*.rs' -print; find scripts tests -type f \( -name '*.py' -o -name '*.fish' \) -print; printf '%s\n' Cargo.toml Cargo.lock; } | sort | xargs sha256sum | sha256sum
  ```

- Linux x86_64；rustc 1.99.1（58cf8f9c3，Arch Linux）；离线 UI 检查使用 GPUI TestPlatform / TestTextSystem。
- `cargo build --release --example perf_native --features native-profiler` 成功；二进制 SHA-256：`b8383f0fe25a4ed9a4cb33ed52f46db1c789720b8b0ab278ae358f126c5aea83`。**仅构建，未启动真实窗口。**

## 本地验证

- 默认 `cargo test --all-targets`：389 项通过；增加 `--features native-profiler`：418 项通过，其中 `perf_native` 27 项。
- 默认 / profiler Clippy（`-D warnings`）、格式、fish 语法及差异空白检查通过。
- `tests/` Python 自检 50 项、`scripts/` 14 项，共 64 项通过；fake probe 子进程不访问桌面。
- 同帧聚合自检区分 selected-ready 与 all-ready，拒绝跨帧拼接、旧 committed snapshot / point、缺失 / 重复 view、错误选择、不兼容布局、空白及未完成索引。配对测试覆盖过期观察丢弃、未来观察保留和缺失；无主动 invalidation 的 idle 要求 30 tick 无 draw。
- 正式 AppShell / probe 在 TestPlatform 中完成左右 / 上下两方向的动作序列、30 次滚动、改宽、字体和 30 次强制绘制；验证共享文本、两个预定独立 point 与 peer scroll。测试使用小 fixture，手工构造提交 frame 检查 capture / 配对 / 状态，不是原生时间测量。
- Python 自检覆盖 73 阶段矩阵、样本 / 方向 / release / 渲染器身份、逐项选中 view 旧门限、缺失 / 非有限 / 乱序数据、idle、子进程失败、部分报告保留、STOP 与首错停止。聚合 ready 为 350 ms 的合成报告可保留为基线，不被错套成 selected-ready；并非实测结果。
- 两轮只读审查完成；发现的临时目录初始化、样本身份与独立 point 漏检已修复并复审。

## 结论与未测边界

本轮完成 `split-perf` 自动工具及离线回归，执行方式见[验收流程](../acceptance.md#阶段-c定向原生性能)，已批准口径见[性能预算](../performance.md)。原生批次计划覆盖两方向、mixed / single-ascii 各三轮；无需用户重复功能 / IME 清单，不截图、不改变输入法、不注入 OS 按键。

**尚未取得本轮宿主执行同意，未运行原生测试。** 没有新的原生字体 / CPU 提交 / GPU 或实际屏幕呈现结论，也未复测六类 headless 时延。先前约 350 ms 的双 view 聚合 headless 风险保持未解决；聚合基线没有新数值门限，不意味着阶段 C 整体通过。旧功能、IME 与性能记录保持冻结，不由本记录覆盖。
