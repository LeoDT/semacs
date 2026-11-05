# E3 换行修复后的单窗口原生复测：首屏超限并及时停止

## 授权与身份

用户批准仅运行 1 个约 8 MiB mixed 原生窗口。助手工具环境没有 GPU 设备 / Wayland 会话变量，因此由用户在真实桌面终端执行 `.pi/tmp/e3-wrap-native-once.fish`，回传目录 `.pi/tmp/e3-wrap-native-host-01/`。本轮只读核对原始目录，没有重跑、改样本、调整预算或追加字体 / CPU 探针。

- HEAD：`138995b31a615833e04a729e8d619c829b9667c4`，含未提交 E3 / Lilex 及 [换行与停止修复](e3-wrap-fix-local.md)。
- 登记本记录前，回传 `source.patch` 与当前 tracked + untracked 差异逐字节一致；SHA-256 `d9b66cc21b4713f1a5bf0187c4b08a59a611282b8f3e7ea3e0a2ecee128c619e`。
- 实际运行的 release / native-profiler `perf_native` SHA-256：`9cc114e569bcaccae90f18f6841d1a76ad9402aa44f161909859348a04cdbbeb`；Python 驱动：`4731ee78143e015e79a00eb4e0464fb9ae5e292cd80027784149c95eb62f75a8`；`src/display.rs`：`595b4d9114439645fee31aeb5775312ab68c9c94a571f69f8f7bfc921b4e5357`。
- mixed 为 8,388,648 字节，SHA-256 `1bbb39412c50be06cda0898f541587c1bda65d801c223d6f4d1a9269a6e5c235`；隔离配置为空。运行后 identity-check 及回传后再次校验全部通过。
- 硬件路径：AMD Ryzen 7 7700 集显、RADV / Mesa 26.2.5，`is_software_emulated=false`；Lilex 16 px / 24 px，软换行开启。实际正文尺寸 744×1312，不同于原失败批次的 738×1306。

## 实测与停止范围

唯一 CSV 为 `native-mixed/mixed-1.csv`，只有 **1 行 open 失败数据**：

| 指标 | 实测 | 适用门限 / 解释 |
|---|---:|---|
| 首份场景 / 首次当前正文提交 | **260.085 ms** | 首次正文 ≤250 ms，超限 |
| 完整 ready 提交 | 空 | 尚未观察到，不能填 0 或判通过 |
| 单次 draw | 7.826 ms | ≤16 ms |
| CPU submit | 5.294 ms | 不是 GPU 执行 / 实际呈现 |
| observed_elapsed | 281.763 ms | 后续观察时间，不是首次提交时间 |
| 可见行 / 预览索引行 | 56 / 57 | 正文可见，尚非完整索引 |
| scanned_bytes | 2,966 | 预览扫描；全文 8,388,648 字节 |
| blank_frames | 0 | 本次唯一观测帧无空白正文 |

CSV 与 stdout 的原因一致：`open: first_submit_ms=260.085 ms > 250.000 ms`。260.085 ms 是已经观察到的真实 CPU 提交边界，而非因迟到回调把 281.763 ms 冒充提交时间。

probe 保存 `status=failed`、`completed=false; budget_passed=false` 的部分报告，非零退出；滚动、改宽、字体、输入、forced / idle 均未启动，没有第二个窗口。probe stderr 为空；Python stderr 是非零退出触发的 RuntimeError，父 / 子 STOP 均保留。environment 为 completed=false、restored=true；配置及 fixture 保持原字节。

## 结论与下一步

此次 **未通过原生性能预算**，但已经验证当前阶段超限后及时停止、保留未测边界、不推进后续动作的原生路径。

这不是原先约 20 秒的完整 ready 指标。因更早的首屏失败而停止，完整 ready、resize / font 及其余矩阵全未测；不能说原生全文性能已修复，也不能说仍需 20 秒。draw / submit 的局部计时无法解释首屏其余耗时，没有 CPU 栈或冷启动分段计时，不直接归因字体、shaping、GPU 或调度抖动，也不靠重跑碰通过。

下一步先准备冷首屏关键区间的最小诊断，区分窗口建立、首屏准备 / 字体 shaping 和提交前等待；新原生运行另行授权，预算与失败目录保持不变。E3 未收尾，F 不启动。
