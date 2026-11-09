# Magit 样式、M-x 历史与 scratch 退出：本地验证

## 范围与源码身份

2026-11-09，本次工作区基于 `693eb3890f99a2aeeff2731f3f2d5f2dd40df177`，尚未提交。覆盖已批准的 Magit 样式细分与 view-local 高亮、Vertico 式 M-x 历史排序和持久化，以及普通非文件 buffer 的退出豁免；不增加 Magit 内容或操作范围。

- diff 批次源码 patch SHA-256：`9290136d26a7bfe3a3d8ebe5c8b6cdca7acd3ca66f0bcb68e4fb7e3fff69c4cf`。
- status 批次源码身份 SHA-256：`23ed348acdbc30d8f410f15211223d74291079e5e25571c95977e7b04cc07f31`。
- 新增且未跟踪的 `src/command_history.rs` 单独补充 SHA-256：`cd1e11deeb85104fcbab37f6bd287c0e961504df8fdef361f2b3ffb91078244d`。
- release `perf_diff` / `perf_status` 二进制 SHA-256 分别为 `cfd8c24e3766ec62dfcb9ce34c64322b86d78681b8cf6d4dcfa02bba9e3bca4a` / `b489803b5ec7606632def944f88cf5ef4141e738adbf2fd7610ef66b4254f301`。
- 驱动 `scripts/perf_diff.py` / `scripts/perf_status.py` SHA-256 分别为 `28a66ab6f88486e8db196b4181280105dc89e10c8f66873d728e7110e3f2b536` / `685a6ec801875a0b4351dccba5663d5723508c366ef17fc990e394b4a90b3ac2`。

两项专项顺序执行期间源码 / 二进制身份检查通过；批次结束后仅更新本记录、索引及工作计划。

## 已取得的结果

- `cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` 通过；JS 示例、类型声明及用户配置副本通过 tsc，Alucard 配置副本通过 Rust / QuickJS 实际加载。
- 一次集中审查指出稳定帧可能线性扫描整个当前 section 的字体范围；已缓存按样式范围身份、section 和有效字体属性确定的正 / 负结果，并覆盖缓存命中、换色复用、字体 / 范围失效及双 view 几何回归。
- 获用户同意重跑后，`scripts/perf_diff.py .pi/tmp/inserted-diff-perf-frozen-20261109-101127` 完成约 256 KiB / 2 MiB patch、各三轮 staged / unstaged。144 条观测的正文及折叠投影核对均通过，没有 missing observations 或 blank frames。
- `scripts/perf_status.py .pi/tmp/inserted-status-perf-frozen-20261109-101127` 完成 64 / 4096 条清单、各三轮；66 条观测均验证成功，包括刷新、折叠、双 view、随包 JS stage / unstage 与独立 index 核对。

环境为 Linux x86_64、TestPlatform / TestTextSystem，release 优化构建；专项使用隔离配置和可丢弃 Git fixture，不加载日常配置，不启动桌面。OS 页缓存未受控，status 每轮为新进程且没有显式字体预热。完整环境、源码身份、fixture、日志和 CSV 保留在上述两个批次目录中，不入库原始日志。

## 失败记录与边界

最初 `.pi/tmp/inserted-diff-perf` 和 `.pi/tmp/inserted-status-perf` 批次因助手并行运行格式化，触发批次源码指纹变化而停止；该批次保留，不算通过。向用户报告并取得明确同意后，才冻结源码、使用上述新目录顺序重跑。

两项重跑均为 headless 功能核对及 CPU 基线，`acceptance=false`；diff 的 `budget_passed=null`，status 没有已建立的专项延迟门限，不能解释为总体或原生性能预算通过。没有测量真实字体、平台 IME、GPU 或屏幕呈现，也未量化自定义 Alucard 配置下的性能。用户提供的两张截图来自 Emacs，仅作参照；semacs 新样式的真实窗口观感仍待用户核对。
