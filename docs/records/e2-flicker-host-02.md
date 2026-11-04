# E2 反馈修复：第二批宿主复核

## 身份与范围

- 用户授权一个闪烁目视窗口，以及 Markdown / HTML 各三轮原生 CPU 测试，回传 `.pi/tmp/e2-host-flicker-02` 和 `.pi/tmp/e2-host-syntax-perf-02`。助手只读核对，没有重跑宿主、调用会改写人工结果的 verify，或更改回传产物。
- 两批均为基线 `8e680a0d78b35634ad278d6ff27664dd2b054371` 加当批未提交差异；完整 `source.patch` SHA-256 同为 `4b991fb166aa49c1946085872279c495fa9acd51f5a4269a0d282d29be53f0b0`。归档前重建当前 tracked / untracked patch，与两份快照逐字节一致。
- Linux x86_64 / `7.2.11-arch1-1`，Rust `1.99.1`，Hyprland / Wayland，`LANG=en_US.UTF-8`。fontconfig monospace 为 Noto Sans CJK SC；不据此宣称逐字形实际字体命中。
- 人工应用 SHA-256 `b1237cdcf685ee9ca7f3958be9b3035b5432d51304e8ae92601bcbb3e3e54643`，与 [修复本地记录](e2-flicker-fix-local.md) 的默认 release 产物相同。
- 原生 probe SHA-256 `ec17dbfb480b8e35d943d0e7378d34c8cde5520fb504012b6627c0d9ff6cce4b`，驱动 SHA-256 `46f08ab272224f8f2e65392ec90f8565d0e3b775ccfc4658b025a95e6e3d781f`；均与本地文件独立核对一致。性能批次顶层另记录了启用 profiler 构建的应用身份，测量对象是 probe，不混用两者。
- 六轮均无软件渲染降级，报告 RADV RAPHAEL_MENDOCINO / Mesa 26.2.5。两份 fixture 和统一 manifest 独立核对一致；manifest SHA-256 `6fd88190ad63b5f91fd7b9e8a909dc5fc201385e7c6b624ba0cc8134e24adca1`。两套原生隔离配置均为空。

## 闪烁目视复核

`flicker-markdown`、`flicker-html`、`flicker-shared-history` 三组用户观察全部为 yes；人工结果 completed，全部正文 / 配置字节核对为真。助手另将两份正文及配置与 fixture 定义逐字节比对，均完全恢复初始字节。

这些观察支持本批约 256 KiB Markdown / HTML 的 ASCII 连续输入 / 删除、共享显示及历史操作中，未改动区域不再整片闪回普通色、没有颜色位移或空白正文；刚输入内容的暂态普通色仍是批准行为。它不是助手独立目视复现或逐帧屏幕测量。

备注为空，未取得人工窗口精确尺寸或额外描述；本批没有 IME 操作、截图或性能测量。首批多语言 / IME 的五组观察仍按 [首批记录](e2-languages-host-01.md) 的范围解释，不将此次复核扩大为真实 IME 时延通过。

## 原生 CPU 批次

Markdown 为 262,256 bytes，HTML 为 262,238 bytes，各三轮，六轮均 completed / restored。逐份 CSV 用只读校验入口重核，合计 228 行，全部通过模式 / fixture / 阶段 / buffer / 版本 / point / bytes / 文本确认 / 可见正文 / 索引 / 语法身份及适用门限检查。

- 各轮均覆盖六次在前一版本语法 pending 时的输入；这不证明 worker 线程实际并行时刻。
- RET 清理输入点前空格的预期已匹配：Markdown newline 为 262,256 bytes / point 88，Join 为 262,255 / 87；HTML 对应为 262,238 / 48 和 262,237 / 47。最终 `TextVersion(68)` 完成当前语法结果，旧工具失败未再出现。
- 各轮均有 30 次稳定 forced draw 和 30 个 idle tick；合计 180 次 forced draw、180 个 idle tick。缺失观测、空白帧及 idle draw 均为 0，所有逐阶段文本确认均为 true，stderr 均为空。
- 实际正文尺寸：Markdown 三轮均 740×1306 px，HTML 三轮均 741×1306 px；不将请求的 960×640 当作实际尺寸。

以下为三轮范围，单位 ms；单键合并 insert / delete / newline / join，批量合并 32 键插入 / 删除。每轮分别通过适用预算，不以均值代替逐次判定。

| 指标 | Markdown | HTML |
|---|---|---|
| 冷打开首次正文提交 | 119.478–138.542 | 136.001–141.360 |
| 冷打开完整布局提交 | 139.192–162.600 | 157.225–161.557 |
| 单键同步动作 | 0.021–0.668 | 0.021–0.778 |
| 单键首次 / 完整布局提交 | 1.036–4.093 | 0.940–3.533 |
| 32 键同步动作 | 10.166–13.669 | 23.434–27.855 |
| 32 键首次 / 完整布局提交 | 10.624–14.416 | 24.470–29.122 |
| 最后编辑的语法 + 布局提交 | 266.825–300.308 | 100.379–100.528 |
| 稳定 forced 首次提交 | 0.315–0.545 | 0.516–1.209 |
| 全批单次 CPU draw 最大值 | 1.431 | 1.823 |

语法 + 布局就绪仍是没有新增数值门限的独立基线。Markdown 最大 300.308 ms 不套用旧纯布局 300 ms 门限；当时的纯布局已在 14.416 ms 内完成。中间版本未观察到的语法 ready 保持空白，没有冒充 0 ms 或逐版本语法完成。

## 结论与未测边界

本批三组人工闪烁观察、独立字节检查及六轮代表性原生 CPU 测试均通过，支持两项反馈修复，无新发现的必修项。首批原始失败与两批回传目录保持不变，不追改历史结论。

这不是 8 MiB 多语言全矩阵、长期持续输入、真实 IME 时延、GPU 执行或合成器实际呈现通过。人工尺寸未记录的边界保留；既有未测边界不自动追加测试。E2 可按已批准范围收尾，等待用户确认后再讨论 F 的最小 diff 范围，未启动 F。
