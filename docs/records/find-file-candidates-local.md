# find-file 候选排序与信息：本地验证与目视确认

## 范围与身份

2026-11-13，工作区基于 `a41aa55`（`feat: align mouse wheel scrolling and horizontal wrapping with Emacs`），与本文同批提交。覆盖 find-file / write-file 候选的 Vertico history → length → alpha 排序、持久化 file-name history、Marginalia `left` 注解与注解列布局；行为见[保真度](../emacs-fidelity.md#候选式读入阶段-b)。

上游参照为 GNU Emacs 31.1（`subr.el` `add-to-history`、`files.el` `file-size-human-readable`、C locale `format-time-string`）、Marginalia `marginalia-20260821.1231` 与 Vertico `vertico-20260830.123`。只做静态阅读与 batch 对照，不在 semacs 中加载用户日常 Emacs 配置。

环境：Linux x86_64、TestPlatform / TestTextSystem（headless）与临时目录 fixture。用户目视在本机真实窗口完成，未记录窗口尺寸、缩放、字体设置或截图。

## 已取得的结果

- `cargo test --all-targets` 926 项通过；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` 通过。
- 注解对照：在临时 fixture 上，`marginalia-annotate-file` 的输出（去掉 3 个前置对齐空格）与 `file_annotation::file` 逐字符相同，覆盖 setuid 权限串、`12k` / `1.9M` 大小、`2d ago` 相对时间、同月绝对时间与跨年 `"%Y"` 形式；`file-size-human-readable` 12 个取值、`seconds-to-string ... 'expanded 'abbrev` 21 个边界（含 `86399 → 23h 60m`、`0.5 → 0s`）与实现一致。
- 排序对照：用 Vertico `vertico-sort-history-length-alpha` 在 batch 复现 10 个候选、6 条历史的顺序（含重复历史的衰减与目录段加权），与 `files::tests::ranks_like_vertico_history_length_alpha` 的期望一致；pty 中确认 `file-name-history` 保存 minibuffer 原文。
- `examples/perf_candidates`（2000 项目录，release / headless）file-filter ready 4.39–4.61 ms、action 0.14–0.15 ms、候选 2000 项；该项没有批准时延门限。注解成本：2976 / 4869 项目录一次列举约 6.9 / 10 ms，在可取消的后台 worker 内完成，未引入额外缓存（owner 名称按 listing 缓存）。
- 用户在本机真实窗口执行 find-file，目视候选注解、左对齐列与颜色，报告无明显问题。

## 未测边界

- 目视只是概述：未逐项记录 CJK 文件名、窄窗口截断、滚动时注解列变化、不同字号 / 字体下的列对齐，也没有截图。
- 未在真实窗口覆盖 write-file 注解、跨会话历史持久化与非 unix 构建；未重跑原生性能矩阵，该基线不表示预算通过。
- `marginalia-annotate-file` 在 batch 中运行，未在带补全 UI 的交互会话中逐项对照；Marginalia 的累积列宽差异与权限串不着色见契约登记的差异。
