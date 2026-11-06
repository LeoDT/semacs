# F2 status / 单文件写入本地验证与基线

## 源码与环境

本记录只覆盖隔离自动化及 headless CPU 基线，不是宿主、原生性能预算或完整 F2 验收通过记录。

- 基础提交：`6d62064`；测量使用含 F2 累计实现及本批工具的未提交工作区，不把基础提交当成实际二进制的完整源码身份。
- 批次：`.pi/tmp/f2-status-baseline-02/`，六个独立 release 进程；源码文件身份清单 SHA-256：`9359731b6e1bbdd1058343a807c4cf1bf8ab58e42ebe12e3e0de49f580ae21ff`。清单记录 HEAD 及 tracked / untracked 源文件散列，每轮前后复核源码及二进制不变。
- `perf_status` 二进制 SHA-256：`e85027da4d0dbdaf7fe39e9df8d4215868dcfdb6ea3cf8cb2d6d6afe0bbde8e5`；命令为 `cargo build --release --example perf_status`，无 profiler feature。
- Linux `7.2.11-arch1-1` x86_64，AMD Ryzen 7 7700；Rust `1.99.1`、Git `2.56.0`。
- GPUI TestPlatform / TestTextSystem，默认 Lilex 16 px / 行高 24 px，请求窗口 960 × 640，status 不软换行。每轮新建进程，无显式字体预热；打开计时前已绘制普通正文和 M-x 准备界面，不声称覆盖冷命令准备成本。文件缓存不受控，fixture 创建先于测量。

源码身份固定于测量时；后续审查只补测试临时目录父目录创建及文档，不改变测量二进制。完整原始日志、fixture 和报告保留在忽略目录，不作为长期契约。

## 范围与独立核对

入口：`python3 -B scripts/perf_status.py NEW_OUTPUT_DIRECTORY`。仅创建可丢弃 Git 仓库，隔离 Git 环境与配置，不访问日常工作文件，不启动桌面。

| 样本 | Git 文件记录 | 展开叶节点 | 正文 UTF-8 字节 | 展开 / 折叠后的投影行数 |
|---|---:|---:|---:|---:|
| small | 64 | 80 | 1832 | 88 / 56 |
| limit | 4096 | 5120 | 103646 | 5128 / 3080 |

两档均按四分之一构造 untracked、仅 unstaged、仅 staged、两侧修改；两侧修改分别投影到两组，冲突组为空。limit 达到模型记录上限，而非把叶节点数误当记录数；路径长度、特殊 Git 状态等功能边界由既有领域测试覆盖，不外推该性能样本。

每档三轮，每轮依次检查打开、叶节点导航、叶节点 TAB no-op、实际垂直滚动、split、共享折叠、折叠时刷新、展开、stage、unstage、unsplit，共 66 条动作报告：

- 打开 / 刷新走原生命令；写入通过默认 `s` / `u`，由真实随包 JS worker 返回请求，不直调 Rust 接受接口冒充脚本端到端。
- 正文来自已知 fixture 配方，包含完整头部、组顺序 / 数量及每条 XY / 路径；可见行 oracle 独立计算 UTF-8 区间，不调用产品 status parser / renderer / section 构造器。
- 同一场景各 view 的 snapshot、显示 / 布局身份、完整索引、非空正文和投影行映射均核对；双 view 共享折叠，保留独立 point，刷新后折叠保留；写后存活叶节点保持、消失叶节点退到原组 heading。
- stage / unstage 均产生恰好一次成功核心终态和核对 / 安装；每次写后比较全部 `ls-files --stage -z` 条目，不只核对目标字母。stage oracle 是当前磁盘内容，unstage 后恢复原 index；普通编辑 buffer 保持未保存正文。每轮后复核 worktree、HEAD / refs、配置及 oracle 文件不变，不要求 index 缓存字节或新增对象不变。

## 计时结果

单位 ms；每格按第 1 / 2 / 3 轮分别列出，不以均值掩盖波动。`first_ms`、`selected_ready_ms`、`all_ready_ms` 在本批每条动作均落在同次观察，下表列其相同值。

| 样本 | 动作 | 首次目标观察 / 选中 ready / 全部 view ready |
|---|---|---|
| small | open | 51.568 / 51.553 / 51.626 |
| small | refresh（双 view、折叠） | 31.058 / 31.162 / 31.276 |
| small | stage（双 view） | 102.998 / 102.883 / 103.078 |
| small | unstage（双 view） | 102.868 / 102.959 / 103.194 |
| limit | open | 68.287 / 67.937 / 67.776 |
| limit | refresh（双 view、折叠） | 48.486 / 49.025 / 48.204 |
| limit | stage（双 view） | 123.627 / 123.547 / 123.300 |
| limit | unstage（双 view） | 113.645 / 112.657 / 114.845 |

写入端到端包含 JS 返回、核心接受、后台预检 / 写入 / status 核对及投影观察，不把 JS 同步返回当写入成功。上述八组的同步分发均为 0.057–0.126 ms；其余七种本地动作的同步分发范围：small 0.052–0.739 ms、limit 0.062–2.131 ms；对应观察到全部 view ready 分别为 0.099–0.845 ms、0.115–2.238 ms。split / 聚合 ready 没有新增时延门限，不能因超过普通单键同步 2 ms 就套错预算。

采样在 `run_until_parked` 后显式 draw，测试调度器可以合并后台完成及内部 paint，本批均为一次 probe 观察。因此首观测与 ready 相同不证明原生逐帧时序，也不能分解出后台完成前的 UI 响应时间。报告的 `probe_draw_max_ms` 只覆盖显式 draw 调用，不含调度器内部 paint，不是所有 draw 的最大值；不据此判定原生 draw / 滚动 / idle 预算。

## 本地检查与结论

本批普通及 `native-profiler` 的 `cargo test --all-targets`（均含 418 条库测试）、两种 all-targets Clippy `-D warnings`、格式 / diff 检查通过；Python 离线 110 条通过，含新增 7 条 fixture / 报告回归；Rust 新增投影 oracle 单测通过。集中审查无 P1 / P2；P3 为独立运行测试依赖 `.pi/tmp` 已存在，已补父目录创建并定向复测通过。日志见 `.pi/tmp/f2-performance-*-01.log`、`.pi/tmp/f2-performance-review-fix.log`。

保留早期本地编译失败日志（私有测试接口 / 字段假设、宏导入冲突）及修复后通过结果，不作为待调查产品缺陷；首批完整基线 `f2-status-baseline-01` 同样保留，第二批只澄清 probe 字段与采样边界，没有删掉慢样本或新增预热。

结论：批准范围内的 status / 单文件写入本地自动化及两档大清单 / 多 view 基线完成。status / 写入没有批准的端到端时延门限，不套正文 300 ms、JS 100 ms 资源限额或原生 draw 预算宣布性能通过。真实平台字体下新 status / 过时标识的可读性、实际键盘 / 焦点以及原生 CPU、IME、GPU / 实际呈现仍未测；是否开展最小定向宿主验证由计划跟踪并另行征得同意。
