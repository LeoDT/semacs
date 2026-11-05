# F1：mode 与只读 Magit diff 本地验证

## 身份与范围

本记录对应基于 `d6a7ba4` 的 F1 未提交实现及其审查修复（Rust 1.99.1、Git 2.56.0）；行为以 [F1 契约](../emacs-fidelity.md#阶段-f1mode-与只读-magit-diff) 为准。外部行为依据是 [固定 Magit 源码参考](../work/f-magit-reference.md)，不是 Magit GUI 实测。

实现覆盖 buffer mode / 配置期局部键位 / 命令上下文、共享 section 折叠、可见行与截断显示、staged / unstaged Git 读取、diffstat / 导航 / 刷新及正确侧的 RET 版本访问。不包含 Git 写操作或完整 Magit 工作流。

验证使用 GPUI TestPlatform / TestTextSystem 与 `.pi/tmp/` 下的可丢弃仓库，未启动真实桌面、未加载日常配置。Git 集成 fixture 自行建立 index / commit / worktree；这些写入仅为测试准备，不是产品 Git 写能力。

## 最终本地检查

- `cargo test --all-targets`：**560 项通过，0 失败、0 忽略**。
- `cargo test --all-targets --features native-profiler`：**609 项通过，0 失败、0 忽略**。
- 默认与 native-profiler 两套全目标 Clippy（`-D warnings`）、`cargo fmt --check`、`git diff --check` 通过。
- `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts` 与 `node --check src/scripting/bootstrap.js` 通过。

新增回归覆盖局部默认 / 覆盖 / 禁用、跨层前缀 / 注记 / 未知 mode、配置失败回滚与成功重载保留会话、只读与旧输入回调、父子折叠 / 节点消失重现 / 祖先回退、Unicode / 长行 / 隐藏坐标、双 view point / 滚动恢复、staged 与 unstaged 旧新侧、空仓库首个 staged 文件、rename / chmod / binary / symlink 类型转换、非 UTF-8 文件名、linked worktree、外部 driver / textconv 禁用及取消 / 迟到结果隔离。

## 一次集中审查

实现基本完成及自检通过后调用一次 reviewer；发现 5 项 P2，无 P0/P1。主代理完成修复并加入回归，未追加第二次审查：

1. diff 内切换比较保留完整 worktree / git-dir，只替换比较类型；元数据消失不能重新发现父仓库。
2. 打开与 blob 访问校验发起输入会话身份，进入并取消读入、切走再返回不能让迟到请求抢回界面。
3. 大于同步 shaping 阈值的截断长行，水平跟随保持到匹配布局可用；不在只有垂直定位时提前消费 reveal。
4. file-type 变更按删除 / 新增两个 patch block 校验并合成摘要；不因一条 raw T 对应两个 block 而拒绝整个 diff。
5. Git 请求固定 `core.quotePath=true`，raw 路径仍按字节保留，不把合法文件名字节误当成正文 UTF-8 错误。

自检还固定了刷新使用原 git-dir、关闭按需抓取缺失 blob、普通与 mode 键位注记保持有效显示等边界。

## Headless CPU 回归

release `perf_view` / `perf_m1` / `perf_split` 使用六类约 8 MiB 样本，各三轮；split 含 right / below 两方向。内置适用门限检查均通过：

| 入口 | 数据行 | 代表最大值（ms） |
|---|---:|---|
| `perf_view` | 18 | 完整 ready 176.451；resize 176.087；font 177.713；单次输入 front 0.374；输入 ready 184.920；滚动 0.216 |
| `perf_m1` | 540 | action 0.448；front 0.649；ready 180.146 |
| `perf_split` | 432 | action 8.574；front 8.686；双 view all-ready 382.714 |

审查修复后另对 mixed / single-unicode 各三轮 `perf_view` 定向复核，共 6 行：完整 ready 最大 156.609 ms，输入 front 最大 0.517 ms，输入 ready 最大 121.278 ms，通过原适用门限。

上述数据衡量普通编辑路径回归，不是 Git 进程耗时、大 diff 容量或 diff 原生输入性能验收。双 view all-ready 沿用独立聚合观察口径，没有新增数值门限，不将 382.714 ms 套用单 view 300 ms 预算，也不据此重开历史非阻塞事项。M1 unsettled-input ready 仍仅代表末键恢复，不是持续原生逐键时延。

本地产物索引：`.pi/tmp/f-verified-{default,feature,clippy,feature-clippy}.log`、`f-perf-{view,m1,split}-02.csv`、`f-post-review-perf-view.csv`；临时产物不是跨会话有效要求的唯一来源。

## 未测边界与下一步

没有运行真实 Wayland、平台键盘 / IME、原生字体、GPU / 实际呈现或原生性能矩阵；没有声明 GUI 手感、配色可读性、折叠指示或长行水平查看已获人工确认。E3 已接受暂缓的问题与原预算不变。

建议下一步只做隔离配置、可丢弃仓库、一个窗口内的 diff 定向人工观察（含分屏），具体范围先取得用户同意。授权与阶段状态统一在 [实施计划](../plan.md) 维护，不把本地完成写成整个 F 已验收。
