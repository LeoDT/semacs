# 阶段 D：最小脚本接入本地验证

## 身份与范围

- 基于 `9e904c2ec841304d9066b4778d18bb6f9315d369` 的未提交工作区；本记录覆盖实现及集中审查修复后的版本，不代表用户已确认阶段收尾。
- 源文件集合：Git 可见的 `Cargo.toml`、`Cargo.lock` 以及 `src/`、`tests/`、`examples/`、`types/` 下文件，共 133 个。按路径排序，对每项依次加入 UTF-8 路径、NUL、原始文件内容、NUL，计算 SHA-256：`aee0957529b03c3c5399b4a013b2583f07b89ef1a49ac74a6997b8f1c2b18413`。
- 环境：Linux x86_64，AMD Ryzen 7 7700，16 个逻辑 CPU；Rust 1.99.1（Arch Linux），gpui-kit 0.8.1，rquickjs 0.14.0。
- 仅本地 Rust、TestPlatform / TestTextSystem 与静态类型检查；未读取用户真实配置、未启动原生桌面、未测真实 IME / GPU / 实际呈现。临时 fixture 和日志位于 `.pi/tmp/`。

## 功能与自检

以下均通过：

- `cargo test --all-targets`：合计 **425** 项测试，包括普通 Rust 和 GPUI headless 测试。
- `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check`。
- `cargo check --all-targets --features native-profiler`：仅确认 feature 编译兼容，不表示该 feature 的全量测试或原生性能通过。
- `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020 examples/init.js types/semacs.d.ts`：示例与宿主声明通过静态检查，TypeScript 不是产品运行依赖。

自动回归覆盖配置缺失 / 非 UTF-8 / 源码超限、启动失败默认值、重载成功替换与失败回滚、命令统一名称查询及 `M-x` / 多键调用、键位覆盖 / 取消 / 别名和前缀冲突、仅正文生效；同时覆盖 JS 异常 / 非字符串 / Promise、执行 / 内存 / 栈 / 文本 / 注册数量与队列限额，以及共享 buffer 的一次编辑 / undo / redo、只读 / IME / pending 屏障、原目标 / 输入会话 / 文本版本 / region 代次 / 配置代次隔离。

集中审查发现的两项问题已修复并加入回归：point / 共享 mark 改变后恢复原值不能使旧结果重新有效；祖先取消标记会覆盖已显式取消的后代，且不依赖声明顺序。无第二次 reviewer 调用；修复由主代理复跑全量检查。

执行期限测试证明本次循环 / 资源错误能被隔离，不证明任意内建操作都可硬抢占，也不构成安全沙箱证据。

## Headless CPU 回归

release，960×640，初始 monospace 16px / 24px，软换行；沿用现有六类约 8 MiB 样本，每类三轮。使用既有 `perf_view`、`perf_m1` 和 `perf_candidates`，不修改预算。下面是该批所有记录的最大值，逐项记录均满足适用门限，不以平均值掩盖失败。

| `perf_view` 指标 | 最大值 ms |
|---|---:|
| 冷首帧 | 0.756 |
| 初始完整就绪 | 154.607 |
| 改宽 / 字体完整就绪 | 154.911 / 155.389 |
| 滚动更新 | 0.261 |
| 稳定 forced frame / empty draw 均值 | 0.066 / 0.065 |
| 插入 / 后删 / 换行 / 合行 front | 0.462 / 0.612 / 0.656 / 0.757 |
| 插入 / 后删 / 换行 / 合行 ready | 168.768 / 167.635 / 167.799 / 167.491 |

`perf_m1` 六类各三轮，共 540 条命令链 / unsettled-input 摘要记录，全部完成；最大 front 1.017 ms、最大已报告 ready 161.866 ms。1024 次 unsettled-input 的 ready 仅对应末键恢复，不是逐键完整 ready 或原生持续输入验收。

`perf_candidates` 在新建的 2000 项文件 fixture 上完成三轮命令 / 文件过滤与导航，共 12 条记录；最大 action 0.270 ms、最大 ready 4.396 ms。此项没有新增时延门限，也没有覆盖 256 个脚本命令的满额候选集合。

原始结果保留在本地 `.pi/tmp/d-closeout-perf-{view,m1,candidates}.csv`；三份对应 stderr 为空。较早自检批次不替代本批最终源码身份。该批没有重跑阶段 C 多 view 性能矩阵，也不把这些 headless 数字外推为原生字体、IME、GPU、实际呈现或运行中任意脚本的性能保证。

## 结论与下一步

最小 JS 接入在上述本地范围内通过。宿主首次显示是否无字体闪变、真实键盘配置及原生重载交互尚未验证；如用户需要，先单独取得同意，再使用隔离配置和可丢弃文本准备最小冒烟入口。阶段收尾与下一步以 `docs/plan.md` 为准。
