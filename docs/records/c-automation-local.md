# 阶段 C：验收自动化与多 view 基线

## 身份与环境

- 未提交工作树，基于 `d7b0209775855785dbfe8acfc535deb2e78784e5`。
- Rust / Python / fish / Cargo 源码身份汇总：`fee7012758af811fb2bd35e5dc25d257a0425cfdb3c28098c9b622e2d57755a1`。包含新增文件，不包含文档、临时产物或用户原有的 `.pi/settings.json` 删除：

  ```sh
  { find src tests examples -type f -name '*.rs' -print; find scripts tests -type f \( -name '*.py' -o -name '*.fish' \) -print; printf '%s\n' Cargo.toml Cargo.lock; } | sort | xargs sha256sum | sha256sum
  ```

- Linux x86_64，rustc 1.99.1（58cf8f9c3，Arch Linux）；GPUI TestPlatform / TestTextSystem，性能使用 release。
- `perf_split` 二进制 SHA-256：`69af7224e255b5868325d223eccf819b337282db845847d149fd28713cc90649`。
- `native_split_ime` 二进制 SHA-256：`0f9e32d89024a200453e9a458c3e11c84cfc8a67f2f3fde98c708b19fa4dddf9`。仅完成编译，未启动真实窗口。
- mixed 种子 8,388,648 字节，SHA-256：`1bbb39412c50be06cda0898f541587c1bda65d801c223d6f4d1a9269a6e5c235`。

本记录补充而不改写[先前本地证据](c-split-local.md)。有效行为见[契约](../emacs-fidelity.md#阶段-cbuffer-与-view)，执行入口见[验收流程](../acceptance.md#阶段-c自动功能回归与多-view-基线)。

## 自动检查

- Rust `cargo test --all-targets`：389 项通过；默认及 profiler feature Clippy、格式及差异空白检查通过。
- `scripts/` Python 自检 14 项、`tests/` Python 自检 38 项通过；fish 入口语法检查通过，均未访问桌面。
- 在已有回归之上增加嵌套三个正文 view 加 minibuffer 的平台输入矩阵：各目标预编辑、caret 范围、取消、再次预编辑、提交、共享正文与非目标无 marked、原 view / point / mark / scroll 恢复，以及旧 minibuffer 回调隔离均自动断言。不是让用户再逐项执行功能清单。
- 新 probe 的 18 阶段判定有离线单测；driver 自检覆盖完整握手顺序、完整报告与独立状态校验、失焦拒绝截图、整 monitor 截图参数、原窗口消失、失败终止 / 恢复焦点及新目录不可覆盖。
- 只读审查发现的全局注键竞态、恢复假成功、minibuffer 中间状态漏检和 popup 截图裁剪问题已处理：新入口不全局注键、不代改输入法；恢复焦点失败不能报告完成；显式检查中间 marked / committed；截图范围明确为 probe 所在整块显示器。

## 双 view headless 基线

`perf_split` 覆盖六类约 8 MiB 样本，左右 / 上下两种双 view 布局，每种组合三轮，共 36 个窗口、432 条阶段数据。自动校验共享文本、point / scroll 隔离、跨 view history、非空正文和最终兼容布局；初始 960×640、monospace 16px / 24px，改宽到 720×480、字体到 bold 18px / 27px。

工具开发期间首次完整基线及定稿复测均完成上述范围；首次最大双 view all-ready 为 350.055 ms。下面是定稿复测各阶段跨样本 / 方向 / 轮次的最大值，单位 ms：

| 阶段 | 同步派发 | front | 两个 view 全部 ready |
|---|---:|---:|---:|
| split + 选择新 view | — | — | 326.863 |
| 插入 | 0.324 | 0.452 | 341.886 |
| 后删 | 0.469 | 0.592 | 344.386 |
| 换行 | 0.436 | 0.557 | 0.558 |
| 合行 | 0.434 | 0.556 | 0.557 |
| 在另一 view undo | 0.322 | 0.443 | 339.930 |
| 在另一 view redo | 0.431 | 0.553 | 343.654 |
| 同回调 32 键输入 | 9.314 | 9.440 | 331.657 |
| 改宽 | 0.379 | 0.475 | 309.886 |
| 字体变化 | 0.236 | 0.307 | 310.107 |

稳定 forced frame 与 empty draw 的每轮 30 次均值，各自跨轮最大值均为 0.071 ms。准备与断言中的全文比对不计入上述测量段；所有耗时均为墙钟，不是 CPU 周期或 GPU 时间。

**这些是新场景的基线，不是新增门限通过结论。** 两个 view 全部 ready 的口径与旧单 view 固定尺寸预算不同；其最大值高于旧单 view 的 300 ms 数值，不能隐去或据此调整原预算，也不能把不同口径直接标为超限或通过。保留该风险，待核对宿主时序与多 view 指标适用条件。本轮不代替旧单 view 性能回归。

## 需要宿主的最小范围

新增 `split-ime` 入口自动完成分屏 / 选择、原目标检查、minibuffer 恢复、保存和磁盘字节验证。用户仅在右正文、左正文、minibuffer 三个位置按窗口提示完成“ni → Escape，再 ni → 提交你”。这些是真实输入法操作，不是重复人工验证编辑功能；其他准备命令使用生产路由中的合成 GPUI 输入。

驱动不全局注键、不更改输入法设置或状态；恢复原窗口焦点前校验其存在，恢复后再次核对。六张候选窗截图覆盖测试所在整块 monitor，运行前必须说明隐私范围并让用户隐藏敏感窗口。候选视觉状态仍为待离线审查，不能由自动成功标记代替。

**尚未运行宿主入口**：真实 Wayland / 输入法候选窗、原生多 view 性能与 GPU 均未在本轮验证。当前完成的是自动化工具、离线自检及上述 headless 基线，不是阶段 C 宿主验收完成。
