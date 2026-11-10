# F5 上游参照：提交历史与提交整理

本文保留固定上游的核对入口，不维护产品能力或实现进度。有效行为与批准范围见 [F5 契约](../emacs-fidelity.md#阶段-f5本地提交历史与提交整理)，当前状态见[计划](../plan.md)，外部运行证据见 [F5 对照结果](../../tests/emacs/RESULTS.md#f5magit-提交与历史参照)。外部参照不表示 semacs 通过验收。

## 固定来源与观察边界

- Magit：本地 `/home/leodt/ghq/github.com/magit/magit`，`942d61d5de37c89a3a7c6cf693696b900b1d97b0`，`v4.7.1-74-g942d61d5`，调查时工作区干净。
- Emacs 31.1、Git 2.56.0；显式加载该 checkout 和指定依赖，不加载日常 init 或整个 package 目录。依赖身份与可复现入口见 [对照说明](../../tests/emacs/README.md#f5magit-提交与历史参照)。
- 已静态阅读下列入口；另运行 `tests/emacs/f5-magit.el` 的 batch ERT。源码推演、替换查询 / 启动器的直接调用、真实 Git 子进程和 batch revision buffer 分别解释，不冒充真实 command loop / GUI、完整 hook / 签名生命周期或 semacs 通过证据。
- 不复制或翻译上游实现，不引入 Magit 为运行依赖。既有 editor / process 生命周期参照仍见 [F3](f3-window-commit.md)。

## 上游行为

| 主题 | 源码入口与结论 |
|---|---|
| commit 菜单 | `magit-commit.el:117–166`：`c` 打开菜单，`c c` create、`c a` amend、`c w` reword、`c f` fixup、`c s` squash。默认参数 `--verbose`；存在活动消息 buffer 时先返回该 buffer。其他 infix / suffix 不因本轮最小菜单自动纳入范围。 |
| amend | `magit-commit.el:243–247,441–448`：检查发布历史确认后直接启动 `git commit --amend`，有 staged 则纳入，没有也可编辑原消息；不自动加入 unstaged，也不增加 HEAD / index 快照冻结检查。 |
| reword | `magit-commit.el:250–268`：同样检查发布历史，运行 `commit --amend --only --allow-empty`，保持 HEAD tree、保留 index 的 staged 改动。默认更新 committer date；数值前缀 / 用户选项扩展仍在现有前缀能力之外，不偷偷改为保留原日期。 |
| 发布历史 | `magit-commit.el:441–448`、`magit-git.el:2213–2215`、`magit-branch.el:206–210`：检查 `magit-published-branches`，该快照默认只有 `origin/master`。不能自行等同“任何 remote-tracking ref / upstream”，也不联网判断是否推送过。 |
| fixup / squash | `magit-commit.el:273–303,394–439`：普通入口创建 `commit --fixup=OID --no-edit` / `--squash=OID --no-edit`，不编辑消息、不启动 rebase。启动前与普通 commit 共享 `magit-commit-assert`。 |
| 没有 staged | `magit-commit.el:450–499`：fixup / squash 的 `strict` 不等于仅允许 staged。有已跟踪 unstaged 时仍按默认 verbose 流程展示 diff、询问是否 `--all`，同意才继续选目标；无改动则报 `Nothing staged (or unstaged)`。已有 merge / 进行中 rebase 是另有分支的流程，不能用 staged-only 短路代替。 |
| 目标选择 | `magit-commit.el:63–70,394–439`、`magit-log.el:1776–1888`：`magit-commit-squash-confirm=t`，默认总是进入专用日志；已有 point commit 只提供初始定位。没有初始目标时跳过连续的 fixup! / squash! / amend! 条目。`C-c C-c`、`.`、`e` 选定，`q` / `C-c C-k` 取消并 kill 选择 buffer；`p` 仍是上一 section，`RET` 仍可查看 commit，不是选定。staged companion 在选择期间显示。 |
| 目标检查的非直觉分支 | `magit-commit.el:405–420` 调用 `magit-rebase-interactive-assert`（`magit-sequence.el:712–739`），即使不是 instant 变体，也可能涉及发布确认或目标之后存在 merge 时的继续 / 重选 / 中止查询。这是源码事实，不能自创“没有 rebase 所以无需这些交互”的替代流程；此分支尚未做真实 command-loop 观察。 |
| 日志范围 | `magit-log.el:699–704,1153–1168`：当前分支，无分支则 HEAD；默认 `--graph -n256 --decorate`，不是 upstream 差异范围。Git 按 graph 语义排列，不强行改为按作者日期排序。 |
| 加载更多 | `magit-log.el:919–953,1405–1429`：`+` 将限制加倍，`-` 减半；刷新整个受限历史，不是 skip / offset 替换当前页。默认没有自动滚动加载。 |
| commit 查看 / 返回 | `magit-log.el:1327–1335` 将 commit section 的 RET remap 到 `magit-show-commit`；`magit-diff.el:1581–1626,3058–3094` 默认复用同仓库 revision buffer，读取 `git show -p --format=`，不自行选择 first parent。日志 / revision 沿用临时 window 与 bury；本轮不扩展负前缀等未批准前缀能力。 |
| root / merge | `magit-diff.el:3086–3094,2772–3013`：root 可显示新增内容；merge 可以产生 combined diff（`@@@`、多个 parent 前缀），不是统一回退到第一父提交。`4040–4045` 的 removed-line 判定排除 combined，所以 combined RET 访问 result，并不根据删除前缀选 parent；行号 / 列映射见 `2047–2077`。该快照仅将三组 hunk ranges 标记为 combined，不能据此宣称上游完整支持任意 parent 数；semacs 的已批准 parent / result 访问边界以契约为准。 |
| 生命周期 / 失败 | 复用真实 Git + editor、hooks / 签名、进程输出 / 中断、终态刷新与局部 window 生命周期；`--no-edit` 不代表能绕过 hooks 可能发起的 editor 请求。取消 / 失败不是回滚，不自动重试，不增加预先冻结 index / HEAD。 |

## 证据使用

普通 commit / editor 的继承行为见 [F3 参照](f3-window-commit.md)。batch 对照只覆盖已记录的直接调用、查询捕获及 fixture Git / revision 行为，不代替真实 transient / command loop 或 GUI 观察；semacs 本地测试、审查修复和未测边界另见[本地记录](../records/f5-history-commit-local.md)。
