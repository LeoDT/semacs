# F4：status 内嵌 diff 与单 hunk 操作

已交付 status 的 staged / unstaged 文件展开查看 hunk、point 所在单 hunk 的 `s` / `u`，保留整文件写入及共享 section / 多 view 刷新行为。行为以 [F4 契约](../emacs-fidelity.md#阶段-f4status-内嵌-diff-与单-hunk-index-操作) 为准。

完成一次集中审查、D/F 冲突修复、本地全目标检查和 status / 独立 diff headless 专项；固定身份、结果与未测边界见 [本地记录](../records/f4-status-hunks-local.md)。没有已知阻塞性遗留；助手未启动真实桌面，不将本地基线解释为原生性能或新界面的宿主通过。

用户在本地验证完成后确认已试用、没有明显问题并同意收尾提交。该反馈未附具体操作覆盖或测量数据，不视为完整宿主矩阵或性能验收；本地冻结记录保留当时边界。

本轮不包含部分行、多 section 批量、discard、完整 transient 或特殊类型写入扩展。后续需求仍从 [计划](../plan.md) 确认优先级与范围。
