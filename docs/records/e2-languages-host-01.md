# E2 首批宿主回传：人工观察与原生工具停止

## 身份与环境

- 用户回传 `.pi/tmp/e2-host-languages-01` 与 `.pi/tmp/e2-host-syntax-perf-01`；助手只做离线核对，没有重跑、修改 fixture 或继续宿主测试。
- 两批基线均为 `8e680a0d78b35634ad278d6ff27664dd2b054371` 加当批未提交差异；完整 `source.patch` SHA-256 同为 `e2fd4f88262368bce459879dc3bfb5540e98fa342e0eae6ef7a0d06325f46f96`。诊断前重建当前 tracked / untracked patch，确认与这两份快照逐字节一致。
- Linux x86_64 / `7.2.11-arch1-1`，Rust `1.99.1`，Hyprland / Wayland，`LANG=en_US.UTF-8`。fontconfig 的 monospace 查询为 Noto Sans CJK SC；这不是实际 shaping 中每个字形字体命中的证明。
- 人工应用 SHA-256：`9fb72e9701c1b59e40c68e3ce546d408480f93fa40820c73cc5c2dd3412b0c3b`（当批记录值）。
- 原生 probe SHA-256：`9b4541e3907fcf905615c60a82cccc23a6b0c645b2c59c6e5d8ef406fd0f59ec`；Python 驱动 SHA-256：`07fb363d02e3ce3021402f487d88d1c674e68f2c4a1ee1406df9de65225564f2`。均与诊断时本地文件一致。
- 原生两份 fixture 与统一 manifest 的摘要均独立核对一致；manifest SHA-256 为 `6fd88190ad63b5f91fd7b9e8a909dc5fc201385e7c6b624ba0cc8134e24adca1`。无软件渲染降级，probe 报告 RADV RAPHAEL_MENDOCINO / Mesa 26.2.5。

## 人工批次

五组观察均由用户答 yes：颜色、分屏 / 主题、Markdown 嵌入代码 IME / 历史、HTML 嵌入代码 IME、布局 / 保存。`result.json` 完成，正文及配置字节检查全真；助手又只读比对两份正文与配置，均符合预期，两个注释位置各增加一个“你”，没有额外拼音或配置改动。

这支持当批用户观察到的功能与保存字节，不是自动 IME 状态 / 时延测量。备注为空，未取得人工窗口尺寸或具体输入法名称；没有截图。用户随后补充“大致都没问题，但是打字的时候，着色会闪”，因此不能仅凭五个 yes 宣布 E2 整体收尾或没有可见缺陷。

## 原生批次与停止原因

Markdown 第 1 轮在 `newline` 阶段停止：

`STOP newline: missing exact same-frame target observation`

CSV 已记录 open / insert / delete，newline 为失败行，完整矩阵未执行；Markdown 后两轮及全部 HTML 轮次未开始。环境记录 `completed=false`、`restored=true`，两级 STOP 和原日志均保留。实际正文范围为 741×1306 px，不将请求的 960×640 当作实际窗口尺寸。

离线核查定位到工具对 RET 的错误假设，而不是已经证明同帧采样缺失：

- fixture 输入点在 `const n = |call(42)`，前方有一个空格，原始偏移 88，正文 262,256 bytes。
- 工具将 RET 预测为纯插入 LF，因此预期 262,257 bytes、point 89。
- 既有 `newline` 契约及 `src/editing/indent.rs` 会清理断点两侧空白；此处以 LF 替换一个空格，正确结果仍为 262,256 bytes、point 88。
- 失败行已观察到当前 `TextVersion(3)`，point 88、索引 262,256 bytes，与上述行为吻合。工具却因错误的预期长度拒绝当前目标，并将不同失败原因合并成同帧观测缺失。

尚未修改工具或放宽断言 / 预算，不能把局部数据或确定的工具问题外推为整批性能通过。后续修复须保持真实 RET 行为、独立精确文本 / point 检查及 fail-closed 身份校验，不为通过而更改产品 newline 语义或静默移动测试位置。

## 着色闪烁诊断与下一步

源码路径与用户现象一致：每次编辑后，`Syntax::highlights` 只暴露与当前快照完全相符的结果；新解析尚未完成时返回 None，随后 worker 又将旧 base 移入后台。绘制因此经历“语法色 → 普通色 → 新语法色”。这是全局撤去已有配色的暂态显示策略，不是已经证明正文或输入坐标错误。

现行 E1 契约明确允许未就绪时暂用普通文本；改善该可见行为需要先确认新的显示策略，再实施。拟讨论方向是将临时绘制颜色与解析完成状态分离，对未编辑部分按编辑映射保留临时颜色，受影响内容暂用普通色；不等待解析阻塞输入，不直接把旧字节坐标或旧树用于新正文，最终结果及语言隔离仍严格绑定当前快照。此段只是待确认方向，不表示已有实现或批准契约。

本批不追加宿主运行。工具修正与显示策略确认 / 修复后，先做本地回归，再对必要的新宿主批次另行征求同意。原生性能、真实 IME 时延、GPU / 实际呈现及未跑样本继续保留未测；F 未启动。
