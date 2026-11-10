# F5：提交历史与提交整理

已交付当前分支 graph 日志与加载更多、固定历史提交及文件版本查看、历史 merge 的只读 combined 与 parent / result 访问、amend / reword、fixup / squash 显式目标选择及最小 commit 菜单。行为以 [F5 契约](../emacs-fidelity.md#阶段-f5本地提交历史与提交整理) 为准，上游来源见[参照入口](../work/f5-history-commit.md)。

完成一次集中审查及修复、默认 / native-profiler 全目标检查和独立 diff headless 回归；没有已知阻塞性遗留。固定源码、结果及当时的未测边界见[本地记录](../records/f5-history-commit-local.md)。助手未启动真实桌面，不将本地基线解释为新界面的宿主或原生性能通过。

用户随后确认“大致试了下没什么问题”，明确没有做完整测试，并同意先收尾提交。该反馈未提供具体操作覆盖或测量数据，不视为完整宿主矩阵、原生键盘 / 焦点 / 显示或性能验收；本地冻结记录保留验证当时的边界，不以本次反馈改写旧结论。

本轮不包含 autosquash rebase、完整 transient、分支 / 远程管理、combined 写入、冲突解决、密码交互或通用脚本 async。后续需求仍从[计划](../plan.md)确认，真实环境验证另行授权，不因阶段收尾自动追加宿主矩阵。
