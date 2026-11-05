# E3 首批宿主人工验证：字体效果未通过

## 身份与范围

用户回传 `.pi/tmp/e3-host-20261105-092757/manual/`。HEAD 为 `138995b31a615833e04a729e8d619c829b9667c4`，包含未提交 E3 实现及工具；登记本记录前核对，回传 `source.patch` 与当时工作区的 tracked + untracked 差异完全一致。

回传二进制 SHA-256 为 `5db9108cefed597f6f6409d127fe13bb2f5177fbbc22a3ca94730b8594a0c038`，与核对时 `target/release/semacs` 一致。release 构建成功，stdout / stderr 均为空。

本批只执行一个人工窗口的第一组观察，不将正常程序退出当作通过。

## 失败与停止

- `observations.tsv` 只有 `styles-fonts\tno`。
- 用户记录：**“没有加粗和斜体，只看到下划线和删除线”**。
- 后续 layout / theme、Rust IME、Markdown IME / 保存未确认。父目录只有 manual，没有 perf；原生性能批次未启动。
- `result.json` 为 completed=false、IME unconfirmed，性能 / IME 时延 / GPU 均未测。
- 保留 STOP.txt、notes.txt、source.patch、原始配置与文件，未修改失败 fixture、重试或调整预算。

## 文件独立核对

两份正文及隔离配置的 SHA-256 都与 `fixture.json` 中的 seed 完全一致：

- `body.rs`：`97b6d7be9a4965d84320783980b34c73b1ecbe465efa861fae80a6a0f390f80f`
- `body.md`：`8f6d1e38c3021fe85fb6e8a7c25e4d9364687ba61c69b30fb219f2578941d4c8`
- `config/semacs/init.js`：`03286d03658b5affd8f1e1598d01916cad38fef18b21c81204b56e9abc4226d5`

因此 verification.log 中 body.rs / body.md 的最终字节 mismatch 是尚未执行 IME 提交、没有达到预期最终状态的结果，不是文件损坏证据。不得为了让 verify 通过而补写“你”。配置中 e3-rich 的 bold / italic / underline / strikethrough 声明仍在。

## 字体环境线索

回传环境为 Arch Linux / Hyprland / Wayland。fc-match 的候选结果：

| 请求 | 返回 |
|---|---|
| monospace | Noto Sans CJK SC Regular |
| monospace:style=Bold | Noto Sans CJK SC Bold |
| monospace:style=Italic | Noto Sans CJK SC Regular |

Italic 请求回到 Regular 可解释斜体不可见的一种可能性，但不能同时证明粗体不可见的原因。fc-match 也不证明 GPUI 实际加载了该 face。尚无实际请求 / 命中 FontId 或运行时字体 trace，根因未确认。

## 状态

E3 宿主验收保持失败 / 未完成，不进入 F。下一步先离线定位字体族解析、fallback 与属性传递；修复方案若涉及平台依赖、外部库或字体策略变化，应先与用户确认。新增真实环境诊断或复测须重新明确范围并取得同意，不沿用本批授权自动重跑。
