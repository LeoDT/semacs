# E3 最小宿主验证入口准备

本文只记录入口准备与离线检查，不是宿主验证结果。用户已同意开始最小验证；实际桌面执行仍由用户在仓库根目录的正常 fish / Wayland 终端完成，范围与停止规则见 [验收流程](../acceptance.md#e3-最小定向验证)。

## 范围

- `styles`：复用人工 fixture / 字节核对工具，一个窗口、两份可丢弃 Rust / Markdown，隔离 rich / plain 主题。四组观察覆盖字体 / 装饰、文字背景 / region / 光标、分屏 / 改宽 / 主题取消，以及用户手动执行的两处 IME 取消 / 提交、Rust undo / redo。仅允许两份正文首行各新增一个“你”，配置与其他正文必须保持原字节。
- `styles-perf`：复用既有原生驱动，mixed 约 8 MiB 三轮，再 Markdown 约 256 KiB 三轮，至多六个顺序窗口。只测代表性原生 CPU / 平台提交，不展开全矩阵，不自动注入 OS 按键、切换输入法或截图。普通 mixed 在每份报告后核对预算，发现失败即停止后续窗口；Markdown 段沿用逐阶段预算及 pending 输入覆盖校验。
- 人工与性能均隔离子进程配置，不读取 / 改写日常配置。性能两段使用空配置，人工使用本批生成的 E3 主题；结果和二进制 / 源码身份分目录保留，不把两者的字体配置混为同一实验。
- 同步纠正现有 `languages` 人工说明中的旧“强调只换颜色”措辞，沿用已批准 E3 粗体 / 斜体默认，不改历史验收记录或预期文本字节。

## 离线检查

- `python3 -B -m unittest discover -s scripts -p 'test_m2_manual.py' -v`：29 项通过，含新增 profile 的独立字节 / 观察校验、失败证据保留、IME 未测边界、样本及窗口范围、混合配置隔离检查。mock 子进程验证 styles 只改变测试子进程 XDG_CONFIG_HOME，保留父环境及日常配置。
- `python3 -B -m unittest discover -s tests -p 'test_*.py'`：68 项通过；使用 mock native / desktop，不运行真实窗口。
- `fish --no-execute scripts/acceptance.fish`、`git diff --check` 通过。
- 生成临时人工 fixture 到 `.pi/tmp/e3-host-tools-fixture`；对其 init.js 和正式类型声明执行 strict tsc 检查通过，没有运行用户配置。
- `cargo build --release --bin semacs --example perf_native --features native-profiler` 通过，仅构建，未执行原生程序。

本批只修改 fixture / fish 入口及其测试和文档，没有改产品 Rust 代码或性能预算。常规工具扩展由主代理验证，没有追加产品阶段 reviewer。

## 待回传

人工四组观察、实际字体效果 / 窗口尺寸 / 输入法、两份最终文件及配置字节，以及性能原始 CSV / 环境 / 停止报告，均尚未取得。真实 IME 时延、GPU 执行和屏幕呈现不在本批测量范围；全文形状及长期输入矩阵未覆盖，不因代表性检查通过而外推。任何失败先保留整个目录并回报，不调优后重测，也不修改 fixture 来通过核对。
