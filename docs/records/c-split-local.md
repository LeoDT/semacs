# 阶段 C：buffer / view 与 split 本地验证

## 源码与环境

- 未提交工作树，基于 `d7b0209775855785dbfe8acfc535deb2e78784e5`。
- 本批 Rust 源码 / Cargo 身份汇总 SHA-256：`acf344f334d3d753edcd8fa8f9624e6a3ce581e76b5c2ba17396a44446163a1b`。计算方式如下，包含新增未跟踪 Rust 文件，不包含文档或用户原有的 `.pi/settings.json` 删除：

  ```sh
  { find src tests examples -type f -name '*.rs' -print; printf '%s\n' Cargo.toml Cargo.lock; } | sort | xargs sha256sum | sha256sum
  ```

- Linux x86_64；rustc 1.99.1（58cf8f9c3，Arch Linux）。功能回归使用 GPUI TestPlatform；性能使用 release、TestPlatform / TestTextSystem，无原生字体、显示服务、真实 IME 或 GPU。
- 行为范围见[阶段 C 契约](../emacs-fidelity.md#阶段-cbuffer-与-view)。本记录不是宿主验收，也不改写既有冻结证据。

## 本地检查

- `cargo test --all-targets`：383 项通过。
- 默认及 `native-profiler` feature 的 `cargo clippy --all-targets -- -D warnings`：通过。
- `cargo fmt --check` 与差异空白检查：通过。
- 新增回归覆盖共享文本 / mark / history / modified / 保存身份、独立 point / scroll、分屏方向和遍历、关闭与 kill 替换、跨 view 保存队列、原 pane 文件打开、minibuffer 取消与焦点恢复，以及组合屏障 / 丢弃 marked / 迟到回调隔离。
- 另覆盖首次 split 绘制不能打断连续输入组、连续分屏与缓存 view 切换不能复用旧尺寸、普通 view 通知不传播到无关 buffer。审查发现的问题已修复并补回归；未新增 Emacs 外部观察记录。

## 既有单 view headless 性能回归

使用 `scripts/perf_sample.py` 生成 8,388,648 字节 mixed 种子；`perf_view` / `perf_m1` 各执行六类样本、每类三轮。初始 960×640，既有字号 / 字体与软换行设置不变；`perf_view` 包含改宽与字体变化，`perf_m1` 包含命令链及 1024 键未等待完整布局的输入段。执行分工及指标边界仍见[验收流程](../acceptance.md)和[性能预算](../performance.md)。

下表为全部运行的最大值，单位 ms：

| 指标 | 最大值 |
|---|---:|
| 冷首帧 | 0.582 |
| 初始完整索引 | 153.192 |
| 滚动 | 0.231 |
| 改宽 / 字体完整索引 | 152.489 / 153.129 |
| 稳定 forced frame / empty draw 均值 | 0.063 / 0.062 |
| 插入 / 后删 / 换行 / 合行 front | 0.397 / 0.392 / 0.377 / 0.515 |
| 插入 / 后删 / 换行 / 合行 ready | 165.733 / 164.739 / 165.525 / 166.708 |
| M1 action / front / 已观测 ready | 0.867 / 0.983 / 160.912 |

`perf_view` 输出 18 条运行数据；`perf_m1` 输出 540 条阶段数据，内置预算断言通过。既有 headless 指标均在原预算内。未等待布局的输入段只测最终有效版本恢复，不将其解释为逐键 ready 或长时间原生持续输入。

## 结论与未测边界

已批准行为的本地实现与上述回归通过；阶段 C 尚未完成宿主验收。

- 多 view 的几何、point、编辑传播和输入生命周期有 headless 功能回归，但本轮性能数据仍来自既有单 view 入口；不是多 view 大文件性能基线。
- 未执行真实 Wayland / X11、原生字体 / GPU、分屏下真实 IME 候选坐标、原生持续输入或人工视觉确认。
- 尚需准备阶段 C 定向宿主验收入口，并另行取得本轮用户同意。后续工作统一见[实施计划](../plan.md)。
