# D1 消息显示与历史：本地验证

## 源码与环境

- 基线：`1c319567976098accfa3c2c8ed3cebae0a9bf2af`，加本轮未提交的 D1 改动。
- `src/` 与 `tests/` 源码摘要：`3d6031a411ff25815d9b501e7680b369164c9ceb5447f39fab5704b02204ab58`。对 `git ls-files --cached --others --exclude-standard src tests` 的去重排序路径，依次合并路径 UTF-8、NUL 和文件内容 SHA-256 的原始摘要，再计算 SHA-256；包括新文件，不包括本文和本地临时材料。
- Linux x86_64，Rust 1.99.1；产品使用 GPUI TestPlatform / TestTextSystem。外部参照为本机 GNU Emacs 31.1、`-Q --batch`，不读取用户配置，不启动桌面。

## 检查与定向回归

- `cargo test --all-targets`：438 项通过。
- `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check`：通过。
- `emacs -Q --batch -l tests/emacs/d1-messages.el -f ert-run-tests-batch-and-exit`：3 项通过；这是独立行为参照，不是产品通过证据。

产品回归覆盖长 / 多行及无空格 Unicode echo 的高度 / mode line 边界，读入注记不改文本和输入快照、折行 / shaping 复用、两秒提示清除和旧任务取消，pending / 读入提示不入日志，单行重复折叠与 1000 行裁剪，只读内容不产生用户 undo / dirty，以及历史的普通 buffer 选择、未选中 pane 显示、关闭重建和另存隔离。

本阶段集中审查发现导出历史期间追加消息可能使已另存 buffer 与磁盘不同却仍为 clean。已区分普通文档的 undo 保存点与生成型 buffer 的文本版本保存点，另存后脱离日志角色；定向回归确认请求后新增内容保留为 modified，再次保存可完整落盘。修复后由主代理重跑上述全部本地检查，未追加 reviewer 调用。

## Headless 性能回归

release `perf_view` / `perf_m1`，六类约 8 MiB 样本，各三轮；`perf_view` 初始 960×640、改宽 720×640、16px/24px 字体改为粗体 18px/27px。以下为本轮各行观测的范围 / 最大值，不是原生预算结论。

| 指标 | 本轮结果（ms） |
|---|---|
| 首帧 | 0.389–0.510 |
| 初始 / 改宽 / 字体完整就绪 | 94.855–153.114 |
| 滚动 | 0.140–0.257 |
| 稳定 forced frame / empty draw | 0.043–0.062 |
| 插入 / 后删 / 换行 / 合行 front | 0.250–0.664 |
| 同上 ready | 0.250–164.467 |

以上已测项目均满足对应 headless 预算。`perf_m1` 共 540 行：命令链同步动作最大 0.565 ms、front 最大 0.699 ms、ready 最大 162.128 ms；未等待布局的输入段 front 最大 0.712 ms、末键恢复最大 0.350 ms。后者不是逐键完整 ready，也不是原生持续输入。

## 结论与边界

D1 本地实现、自检及审查后修复已完成；真实桌面未获本轮授权，尚未验证原生字体下的重叠修复、实际焦点观感、真实 IME、GPU 或原生性能。不能据此宣布 D1 宿主验收完成；下一步仍按实施计划征求定向目视验证同意。
