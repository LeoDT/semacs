# F4 status 内嵌 diff 与单 hunk 操作：本地验证

## 范围与身份

2026-11-10，工作区基于 `21e5716997777852661a875f190a1f4648f041eb`，尚未提交。覆盖已批准的 status 内嵌 staged / unstaged diff、单 hunk stage / unstage，以及原整文件操作和刷新恢复的兼容性；行为见 [F4 契约](../emacs-fidelity.md#阶段-f4status-内嵌-diff-与单-hunk-index-操作)。

上游参照固定为 Magit `942d61d5de37c89a3a7c6cf693696b900b1d97b0`：静态核对 `lisp/magit-diff.el` 的 status 插入、文件初始折叠与 hunk 解析，及 `lisp/magit-apply.el` 的单 hunk patch、起始行调整、apply 参数、失败与刷新。不把静态阅读作为真实 Magit 运行证明；未加载日常 Emacs 配置。

最终专项顺序运行，期间源码 / 二进制身份核对通过；之后仅做文档收尾：

| 项目 | SHA-256 |
|---|---|
| status 源码身份（含未跟踪文件） | `3d014534e44067df96f8df2c406ea987276bd366c5fdd591ceb7172ce621a4f5` |
| release `perf_status` | `4a530cfcb7fd85b1a1dd4af65ccd134340983691f481b09b6d53282db517e8ed` |
| diff 源码 patch | `64306f5061d0b91c5eb193a62b7950266f08203011074e06935b9398c15b85c6` |
| profiler release `perf_diff` | `1140f45315814c2cf196573ec97e2512d723b1e1613b201e2e70f2c7e687282e` |
| `scripts/perf_status.py` | `1ac4bc8f4c498cc10bb3f8a86e5a7920e53048b16082b86d57ab911110a9acaf` |
| `scripts/perf_diff.py` | `28a66ab6f88486e8db196b4181280105dc89e10c8f66873d728e7110e3f2b536` |

环境为 Linux x86_64、内核 `7.2.14-arch1-1`、TestPlatform / TestTextSystem。使用独立配置和可丢弃 Git fixture，不启动桌面、不操作日常仓库；OS 页缓存未受控。

## 已取得的结果

- `cargo test --all-targets` 和对应 Clippy（`-D warnings`）通过；默认 lib 529 项通过。带 `native-profiler` 的全目标测试与 Clippy 也通过，lib 531 项通过。
- `cargo fmt --check`、`git diff --check`、JS 示例 / 类型声明的严格 tsc 检查通过。Python tests 目录 110 项与 status 人工工具的 17 项离线测试通过。
- 真实 Git fixture 核对多 hunk 独立 stage / unstage、前序 hunk 改变行数、操作前 HEAD / index / disk 变化、Git 上下文不匹配失败、新增 / 删除与 unborn、EOF 标记、CRLF、chmod 分离、引用后的原始路径、锁及特殊 index 标记。核对实际 index / worktree / refs 与无关路径，而非仅以 Git 退出码判断。
- App / JS 回归核对默认 `s` / `u` 的单 hunk 语义、共享折叠、各 view 的 point / scroll、hunk 消失回退、异步返回不能改选同一文件的另一 hunk，以及 status 被删除后已接受 patch 仍完成且不安装进替代会话。
- `.pi/tmp/f4-status-headless-02` 完成 64 / 4096 条记录各三轮，66 条观测全部 verified；独立正文 / 投影 oracle 覆盖内嵌 diff、默认文件折叠、文件展开、双 view、刷新和原整文件 JS stage / unstage 的完整 index。单 hunk 写入由上述 Git / App 回归覆盖，不将该基线冒充 hunk 延迟测量。
- `.pi/tmp/f4-diff-headless-01` 完成约 256 KiB / 2 MiB patch 各三轮 staged / unstaged；144 条观测的正文与投影验证通过，无 missing observations 或 blank frames，覆盖共享 parser 的独立 diff 回归。

status 的 selected-ready 基线：64 条记录打开 71.808–72.008 ms、文件展开 0.167–0.176 ms；4096 条记录打开 341.019–341.536 ms、文件展开 0.783–0.997 ms，刷新 315.209–324.235 ms，整文件 stage / unstage 377.117–400.229 ms。它们没有已建立的专项延迟门限，不解释为原生预算通过。

完整身份、fixture、日志和 CSV 留在上述两个批次目录；首次成功的审查前 status 基线另保留于 `.pi/tmp/f4-status-headless-01`，不冒充最终源码结果。

## 集中审查修复与边界

一次集中审查发现：literal pathspec 仍会将删除文件名 / rename source 扩大匹配为同名目录，D/F 合并冲突因此会把 unmerged 记录带入严格 diff parser，导致整个 status 加载失败。主代理补充真实 Git D/F 合并 fixture，先重现失败，再使 status 专用读取在解析前排除 unmerged diff；删除和暂存 rename 两种状态均通过，独立 F1 diff 的冲突拒绝边界仍受回归约束。审查后由主代理完成修复与全量复测，没有追加 reviewer。

两项最终专项均为 headless 功能核对和 CPU 基线，`acceptance=false`，diff 的 `budget_passed=null`；未验证 F4 在真实窗口中的新布局 / 观感、原生输入 / 绘制、字体、IME、GPU 或屏幕呈现，不声明总体性能通过。之前样式插入项已由用户确认目视无问题；这不是 F4 新界面的宿主验证，也不改写旧冻结记录当时的未测结论。
