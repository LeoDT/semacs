# 实施计划：鼠标基础选区与文件路径预填

> **状态：已实施（T1–T5）。** 用户已确认本计划；行为契约见 [保真度契约](../emacs-fidelity.md#鼠标选区) 与[文件读入的默认目录与路径答案](../emacs-fidelity.md#文件读入的默认目录与路径答案)，交付摘要见 [实施计划](../plan.md)，Emacs 31.1 参照观察见 [RESULTS](../../tests/emacs/RESULTS.md#m3鼠标选区与文件读入预填参照)。真实桌面鼠标与候选窗未测。

## 概述

本轮分两条独立的用户可见能力：

1. 鼠标基础选区：单击定位、拖动选择 region、拖出 view 上下边界时自动滚动。本轮不实现多击、Shift 调整和右键操作。
2. 文件路径预填：`C-x C-f`、`C-x C-w` 和无路径 buffer 的 `save-buffer` 初始输入为默认目录，候选列出该目录内容；补充未编辑 RET 与目录输入的 Emacs 语义。

两条能力共享 `docs/emacs-fidelity.md` 的文档更新，但代码与测试互不依赖，可分别实施。

## 需求来源

- 用户要求：支持鼠标选择 region。
- 用户要求：find-file 的输入区应包含当前文件所在目录，候选列出该目录对应文件，行为对齐 Emacs。
- 用户确认：
  - 鼠标先实现基础选区；已核实的双击 / 三击 / Shift / 右键行为记录后另批实现。
  - 接受 Emacs 的单击语义：mark 落在点击位置，region 停用。
  - 自动滚动本轮实现。
  - `find-file`、`write-file`、无路径 `save-buffer` 一致预填目录。
  - 预填目录后直接 `RET` 不尝试打开目录，保留明确提示；登记缺少 dired 的差异。
  - 从 Magit / diff / 历史等生成 buffer 发起文件读入时，默认目录取仓库根。
  - 无路径 buffer 的 `save-buffer` 未编辑 RET 按 Emacs 语义处理。

### 已核实的上游参照

Emacs 31.1 源码与 pty + `xterm-mouse-mode` 观测确认：

- `down-mouse-1` → `mouse-drag-region`；按下即设 mark 并 active；拖动更新 point；松开后普通单击停用 region，拖动保留 active。
- 双击选词、三击选整行（含行尾换行）；`mouse-skip-word` 还区分空白、标点与 symbol。
- `mouse-drag-copy-region` 默认 `nil`；`mouse-shift-adjust-mode` 默认关闭，默认 `S-down-mouse-1` 是 `mouse-appearance-menu`。
- 拖出窗口按 `mouse-scroll-subr` 与 `mouse-scroll-delay`（默认 0.25）滚动。
- 文件读入由 `insert-default-directory` 默认 `t` 决定初始输入为 `abbreviate-file-name(default-directory)`。
- 未编辑 RET 使用 `default-filename`；`write-file` 遇到目录名会追加当前 buffer 的文件名或 buffer 名。

详细观测环境与结果记录在 [外部参照观察](../../tests/emacs/RESULTS.md)；该记录不是 semacs 通过证据。

## 范围

### 目标

- 鼠标左键单击、拖动形成 region，拖出 view 上下边界自动滚动。
- `C-x C-f` / `C-x C-w` / 无路径 `save-buffer` 预填默认目录并列出对应候选。
- 未编辑 RET、目录输入及默认文件名的行为对齐已核实的 Emacs 语义，除明确的 dired 差异。
- 相关 headless 测试、契约文档和未排期事项同步更新。

### 非目标

- 双击选词、三击选行、Shift 调整 region。
- 右键 `mouse-save-then-kill`、`mouse-drag-copy-region`、上下文菜单。
- dired、目录浏览、文件历史 / `M-n` 默认文件名界面。
- 递归目录候选、远程路径、`directory-abbrev-alist`。
- 鼠标拖动移动文本（drag-and-drop）、鼠标滚轮行为调整。
- 修改 `default_directory_for` 在 commit / scripting 等非文件读入场景的既有行为。

## 行为与全局不变量

- **INV-1**：鼠标单击 / 拖动只改变 point、mark 与 region active 状态，不修改文本、modified、undo history、kill ring 或平台 selected range。
- **INV-2**：普通单击后 `point == mark == 点击位置` 且 region 停用；拖动后 mark 固定在按下位置、point 位于当前指针位置、region 保持 active。
- **INV-3**：拖动过程中只打断一次历史组 / 命令链；region 高亮沿用现有 committed point / mark 投影，不启用 delete-selection。
- **INV-4**：IME 组合期间不开始鼠标选区；拖动期间窗口失活或松开后必须清理拖动态与自动滚动任务。
- **INV-5**：自动滚动只在指针位于本 view 的水平范围内、且越过上下边界时发生；仅垂直滚动，停止后不得残留定时器。
- **INV-6**：文件读入的初始输入是带尾部分隔符的缩写目录；候选目录与 `RET` 路径展开基准必须使用同一个目录解析结果。
- **INV-7**：默认目录解析顺序固定为：origin buffer 关联仓库根 → 文件父目录 → 启动目录；不改变其它调用方的 `default_directory_for`。
- **INV-8**：`C-x C-f`、`C-x C-w`、无路径 `save-buffer` 使用同一预填规则；文件候选继续只列直接子项，保留 typed prefix 与现有模糊匹配语义。
- **INV-9**：`write-file` 未编辑 RET 使用当前文件路径；无路径 `save-buffer` 未编辑 RET 使用“默认目录 + buffer 名”；`write-file` 的目录输入追加当前文件名。`find-file` 的目录输入因缺少 dired 只提示，不访问目录。
- **INV-10**：路径缩写只把 `$HOME` 的完整组件前缀替换为 `~`，不得把 `/home/a` 误缩写为 `/home/ab` 的前缀。

## 架构与文件职责

### 已锁定决策

- 鼠标选区复用 `Buffer` 的 point / mark / active 与现有 region 绘制，不引入第二套 selection 模型。
- 单击语义采用用户确认的 Emacs 行为，即使这会改变现有“点击保留旧 mark active”的测试预期。
- 文件预填通过 `ReadSession` 的初始文本实现；prompt 仍是只读前缀，不改成外置标签。
- `ReadSession` 保存本次初始输入，用于判断未编辑 RET；不依赖确认时重新计算目录。
- 新增文件读入专用目录解析入口，不修改 `default_directory_for` 的既有消费者。
- `find-file` 目录输入只提示，不实现 dired；该差异进入保真度文档。

### 推荐方案

- 鼠标指针状态留在 `BufferView`，编辑目标只暴露最小 pointer 入口；自动滚动通过 paint 阶段注册窗口级鼠标事件与 250 ms 定时任务实现。
- 文件目录缩写、尾部分隔符与默认文件名拼接放在 `files` 层，便于单元测试；应用层只负责 origin 目录解析与命令语义。

### 执行时可决定

- `PointerDrag` 的字段命名、私有 helper 拆分、定时任务的取消实现。
- 测试中模拟鼠标事件的辅助函数命名与放置位置。
- 文档段落的精确措辞，只要不改变本计划锁定的行为。

## 任务与依赖

| ID | 任务 | 依赖 | 风险 | 主要范围 |
|----|------|------|------|----------|
| T1 | 鼠标基础选区与单击语义 | 无 | 中 | `src/view.rs`、`src/editing.rs`、view 测试、现有点击断言 |
| T2 | 拖出 view 自动滚动与生命周期清理 | T1 | 高 | `src/view.rs`、view 测试 |
| T3 | 文件读入目录预填与候选目录 | 无 | 中高 | `src/files.rs`、`src/app/{app,candidates,reads,documents,buffer_commands}.rs`、`src/session.rs`、文件 / 候选测试 |
| T4 | 未编辑 RET、目录与默认文件名语义 | T3 | 中高 | `src/app/{reads,documents}.rs`、相关测试 |
| T5 | 契约、参照记录与计划收尾 | T1–T4 | 低 | `docs/emacs-fidelity.md`、`docs/plan.md`、`tests/emacs/RESULTS.md` |

## 并行约束与原子边界

- T1 与 T2 共享 `src/view.rs` 的拖动态与测试文件，T2 必须在 T1 的自检通过后继续；两者不得并行修改同一状态模型。
- T3 与 T4 共享 `ReadSession` 初始文本与文件读入确认路径，按 T3 → T4 顺序完成；T3 结束时必须已能编译并通过新增预填测试。
- T1 / T2 与 T3 / T4 代码互不依赖，可分别推进；但都会修改 `docs/emacs-fidelity.md`，文档合并必须等两边行为定型，避免同一文件并发冲突。
- 任一任务结束时必须恢复完整构建；若中间状态引入未使用 helper，必须由同一任务接入消费者，不能让 `clippy -D warnings` 失败。
- 不得在 T1–T4 中间状态更新“已完成”结论；T5 只做文档一致性收尾。

## 任务详情

### 任务 T1：鼠标基础选区与单击语义

**目标：** 左键单击设置 point / mark 并停用 region；拖动形成 active region。

**行为与验收：**
- [x] 单击任意可命中位置后：point 与 mark 都等于点击 offset，region 停用，文本 / modified / undo / kill ring 不变。
- [x] 从起点向前或向后拖动：mark 固定在按下位置，point 跟随指针，region 范围正确且 active。
- [x] 按下后未跨到不同 buffer 位置即松开：按单击语义结束。
- [x] 在只读 buffer、Magit 生成 buffer 和 minibuffer 中行为一致；只读不阻止 point / mark 移动。（只读与生成只读 buffer 有鼠标用例；minibuffer 共用同一入口，沿用既有 pointer 测试。有内容的 status buffer 未单独覆盖。）
- [x] IME 组合期间按下沿用现有拒绝提示；窗口失活或输入生命周期结束时清理拖动态。
- [x] 现有点击相关测试按新语义更新，不再要求点击保留旧 mark active。

**依赖：** 无。

**原子性：** T1 结束时拖动只覆盖 view 内移动；拖出 view 不滚动但必须能安全结束。完整构建与现有回归通过。

**预计涉及范围：**
- 修改：`src/view.rs`、`src/editing.rs`。
- 测试：新建 `src/view/mouse_tests.rs`；修改 `src/view/split_tests.rs`，复核 `src/view/minibuffer_tests.rs`、`tests/split.rs`、`tests/view.rs`。
- 文档：`docs/emacs-fidelity.md` 增加基础鼠标契约；`docs/plan.md` 记录未实现的多击 / Shift / 右键事项。

**实施要点：**
- `EditTarget` 增加 pointer 入口：开始选区时设置 mark / point 并打断历史组与命令链；移动时只更新 point；单击结束时停用 mark。
- `BufferView` 保存按下位置与是否已移动；移动判定以 hit-test 的 buffer offset 是否变化为准，与 Emacs“跨到不同位置才算拖动”一致。
- 每次 point 变化刷新平台 text selection geometry；region 高亮仍只由现有绘制路径产生。
- 点击后 mark 停在点击位置而不是恢复旧 mark；这是用户确认的行为变更。

**验证方式：**
- [x] `cargo test --lib view::mouse_tests`
- [x] `cargo test --test split --test view`
- [x] `cargo test --lib view::`
- [x] `cargo clippy --all-targets -- -D warnings`

**风险级别：** 中。

**审查重点：** mark / active 生命周期、断点历史组、只读与 minibuffer 一致性、旧测试语义更新是否完整。

### 任务 T2：拖出 view 自动滚动与生命周期清理

**目标：** 指针越过 view 上下边界时持续滚动，同时扩展 region；松开或失活后无残留任务。

**行为与验收：**
- [x] 指针在下边界外时向下滚动，在上边界外时向上滚动；point 跟随到新的可见边界。
- [x] 越界距离较大时先按距离滚动，之后每 250 ms 至少再滚 1 行；滚动受 buffer 首尾限制。
- [x] 指针回到 view 内后停止滚动并恢复正常 point 跟随；离开水平范围或进入其它 pane 时停止自动滚动。
- [x] 在 view 外松开（窗口仍收到事件）结束拖动并保留当前 region；窗口失活结束拖动并取消定时任务。（均有 headless 用例。）
- [x] 自动滚动期间不修改文本、不触发编辑历史，不产生横向滚动。
- [x] 测试使用 `advance_clock` 验证定时滚动与停止，不使用真实等待。

**依赖：** T1。

**原子性：** T2 结束时 T1 的拖动语义、单击语义和现有测试仍通过。

**预计涉及范围：**
- 修改：`src/view.rs`。
- 测试：`src/view/mouse_tests.rs`。
- 文档：`docs/emacs-fidelity.md` 补充自动滚动边界与停止条件。

**实施要点：**
- 拖动态保存最后外部指针位置与唯一滚动 `Task`；所有鼠标 up 入口调用统一 finish 函数，重复调用必须幂等。
- 在 paint 阶段、拖动态存在时注册窗口级 `MouseMoveEvent` / `MouseUpEvent`；元素级 hover 监听继续负责 view 内的即时跟随，窗口级监听只处理外部位置，避免重复更新。
- 可见边界 offset 从当前 wrap index 与 scroll 计算，不依赖可能尚未完成的新布局。
- 指针横向离开本 view 时清除外部位置并停止滚动；窗口激活状态变化时清理拖动态。

**验证方式：**
- [x] `cargo test --lib view::mouse_tests`
- [x] `cargo test --lib view::`
- [x] `cargo clippy --all-targets -- -D warnings`

**风险级别：** 高。

**审查重点：** 定时任务所有权与取消、事件重复消费、失活 / 松开遗漏、sections 与可见边界坐标、滚动到首尾时的 point 夹紧。

**人工验收：** 真实指针的跨 pane 移动、窗口外松开与持续滚动需要宿主环境；取得用户对具体范围的同意后，用一个隔离窗口完成“单击、向前 / 向后拖动、拖出上下边界、松开、窗口失活”的最小检查。未执行时保留未测。

### 任务 T3：文件读入目录预填与候选目录

**目标：** 文件读入初始输入为默认目录，候选列出该目录内容；三类命令一致。

**行为与验收：**
- [x] 从普通文件 buffer 发起 `C-x C-f`：输入区为带尾部分隔符的缩写父目录；候选为该目录直接子项，value 保留该前缀。
- [x] 从 scratch 发起时使用启动目录；从 status / diff / history / 其它关联仓库的生成 buffer 发起时使用仓库根。
- [x] `C-x C-w` 和无路径 `save-buffer` 使用同样的初始目录。
- [x] 继续输入、DEL、TAB、原始输入接受和模糊匹配仍正常；`~` 前缀在确认时正确展开。
- [x] 仅文件读入使用新目录解析；commit / scripting 等既有 `default_directory_for` 行为不变。
- [x] 现有 find-file 测试在显式清空预填输入后保持原意，并新增预填 / 缩写 / 仓库根测试。

**依赖：** 无。

**原子性：** T3 结束时三类命令都能预填并列出候选；未编辑 RET 仍按旧行为处理，由 T4 补齐。若旧行为会产生目录读取错误，测试必须避免走到该路径或明确断旧行为，T4 完成后更新。

**预计涉及范围：**
- 修改：`src/files.rs`、`src/app/buffer_commands.rs`、`src/app/candidates.rs`、`src/app/reads.rs`、`src/app.rs`、`src/app/documents.rs`、`src/session.rs`。
- 测试：`src/files.rs` 单元测试；`tests/support/mod.rs`；`tests/candidates.rs`、`tests/save.rs`、`tests/m2_flows.rs`、`tests/buffers.rs`、`tests/split.rs`；必要时 app 层测试。
- 文档：`docs/emacs-fidelity.md` 候选式读入段落。

**实施要点：**
- `files` 提供目录输入 helper：确保尾部分隔符并按路径组件缩写 `$HOME`；root 保持 `/`。
- `AppShell` 提供文件读入目录解析：repository root → 文件父目录 → startup directory。`Buffer::repository()` 已覆盖 status / diff / history / process 等生成 buffer。
- `ReadSession` 保存 `begin_read` 传入的初始文本并提供只读访问；`C-x C-f` / `C-x C-w` / 无路径 `save-buffer` 计算初始值后传入。
- 候选刷新与确认展开统一使用同一目录解析 helper，避免“预填目录”和“确认基准”不一致。
- 现有 `tests/support::find_file` 在打开读入后先 `C-a C-k` 清空预填，再输入目标路径；依赖相对候选的测试同样显式清空，新增测试覆盖预填本身。

**验证方式：**
- [x] `cargo test --lib files::`
- [x] `cargo test --test candidates --test save --test m2_flows --test buffers --test split`
- [x] `cargo test --lib app::`
- [x] `cargo clippy --all-targets -- -D warnings`

**风险级别：** 中高。

**审查重点：** 默认目录解析顺序、仓库根 vs 文件父目录、`~` 缩写的组件边界、候选 value 与确认展开一致性、测试是否因清空预填而失去覆盖。

### 任务 T4：未编辑 RET、目录与默认文件名语义

**目标：** 预填后的 RET 行为对齐已确认的 Emacs 语义，并登记唯一例外（无 dired）。

**行为与验收：**
- [x] `C-x C-w` 输入仍等于初始目录且目标有文件路径：使用当前文件路径保存，进入既有覆盖确认流程。
- [x] 无路径 buffer 的 `save-buffer` 输入仍等于初始目录：使用“默认目录 + buffer 显示名”作为路径，进入既有写入 / 覆盖流程。
- [x] `write-file` 最终输入以路径分隔符结尾：追加当前 buffer 文件名（无路径时使用显示名）后写入，行为对齐 Emacs。
- [x] `find-file` 最终展开路径是目录：显示 `Select a file or enter a file name`，保留会话，不创建 buffer、不读取目录。
- [x] 编辑过输入后回到与初始串完全相同的文本，仍按未编辑处理；其它非空路径按现有确认流程。（有集成用例。）
- [x] 现有保存 / 覆盖 / 错误处理路径不被绕过；`find-file` 的新文件语义不变。

**依赖：** T3。

**原子性：** T4 结束时文件读入预填、候选、RET 与目录处理形成完整闭环；T3 的测试不残留旧错误预期。

**预计涉及范围：**
- 修改：`src/app/reads.rs`，必要时 `src/app/documents.rs`。
- 测试：`tests/candidates.rs`、`tests/save.rs`、`tests/m2_flows.rs`，必要时新增 app 层测试。
- 文档：`docs/emacs-fidelity.md` 文件与退出段落；登记 dired 暂缺。

**实施要点：**
- 未编辑判定使用 `ReadSession` 保存的初始文本与当前输入的字符串相等。
- `write-file` 目录输入追加目标 buffer 的 `path().file_name()`，无路径时使用 `display_name()`；随后走既有 `write_file` / 覆盖确认。
- 无路径 `save-buffer` 的默认文件名使用 `file_read_directory` 与 `display_name` 拼接，不自行发明安全检查或额外确认。
- `find-file` 的目录检查在 `files::expand` 成功后进行；不改变新文件 / 不存在的接受规则。

**验证方式：**
- [x] `cargo test --test candidates --test save --test m2_flows`
- [x] `cargo test --lib app::`
- [x] `cargo clippy --all-targets -- -D warnings`

**风险级别：** 中高。

**审查重点：** 未编辑判定稳定性、目录输入追加文件名、无路径 buffer 默认名、`find-file` 目录会话保留、覆盖确认不被绕过。

### 任务 T5：契约、参照记录与计划收尾

**目标：** 行为契约、外部参照记录和当前计划与实现一致。

**行为与验收：**
- [x] `docs/emacs-fidelity.md` 只写已实现行为与已批准差异；鼠标多击 / Shift / 右键不写成已支持。
- [x] `docs/plan.md` 保留未排期鼠标行为摘要，并给出本轮交付摘要与必要遗留事项。
- [x] `tests/emacs/RESULTS.md` 记录 Emacs 31.1 pty 鼠标观测的环境、方法和状态转移，明确是外部参照而非 semacs 通过证据。
- [x] 文档之间引用有效，没有把待确认提案写成已生效契约。

**依赖：** T1–T4。

**原子性：** 文档更新完成后，计划、契约与代码状态一致；不包含新的产品行为变更。

**预计涉及范围：**
- 修改：`docs/emacs-fidelity.md`、`docs/plan.md`、`tests/emacs/RESULTS.md`。
- 删除：无。

**验证方式：**
- [x] `git diff --check`
- [x] 人工核对文档链接与契约表述
- [x] `cargo fmt --check`

**风险级别：** 低。

## 阶段检查点

1. **T1 完成**：鼠标单击 / 拖动语义稳定，现有 view 点击测试已更新；运行 T1 定向测试与 clippy。
2. **T1–T2 汇合（鼠标）**：view 内拖动、拖出自动滚动和清理路径全部通过；复核 region 绘制与 sections 投影；文档草稿只描述基础鼠标行为。
3. **T3 完成**：三类命令预填与候选一致，旧测试显式清空预填后保持原意；运行文件 / 候选测试与 clippy。
4. **T3–T4 汇合（文件读入）**：未编辑 RET、目录输入、默认文件名和覆盖确认闭环；运行相关集成测试与 clippy。
5. **T1–T4 汇合后集中审查**：按仓库规则最多一次集中审查，对象为鼠标拖动态 / 定时任务生命周期与文件读入路径语义；审查前完成自检，审查发现的问题修复后由主 Agent 按影响验证，不机械重复全面审查。
6. **T5 与最终验证**：文档一致、完整检查通过；未执行的宿主鼠标检查如实保留为未测。

## 最终验证

- [x] `cargo test --all-targets`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`
- [x] `git diff --check`
- [x] 按影响复核 `cargo test --lib view::`、`cargo test --test candidates --test save --test m2_flows --test buffers --test split`
- [x] 人工鼠标验收（需用户同意）：真实指针单击 / 拖动 / 拖出上下边界 / 松开 / 窗口失活；未获同意或未执行时保留未测。

本轮不预期修改 profiler、Git 读写、性能预算或脚本配置，不默认追加 native-profiler、宿主性能矩阵或完整人工候选流程。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 鼠标全局监听与元素监听重复更新 point / 重复结束拖动 | 统一 finish 幂等；窗口级只处理外部位置，元素级处理内部；测试覆盖重复 up。 |
| 松开或失活遗漏导致定时器继续滚动 | 拖动态只有一个 Task；所有 up / 失活 / 新按下路径清理；测试覆盖释放与失活。 |
| 自动滚动在 sections / 折叠投影下越界或跳过隐藏行 | 使用 wrap index 的可见行计算边界；定向测试 diff / 折叠场景；保留未覆盖边界。 |
| 点击语义改变破坏现有历史 / mark 测试 | T1 明确列出并更新断言；集中审查核对旧测试是否被误删而非改对。 |
| 预填改变候选 value，测试通过清空输入而失去原覆盖 | 新增独立的预填 / 缩写 / 仓库根测试；只对“空输入候选”原意测试清空，不批量削弱断言。 |
| 默认目录取仓库根影响非文件读入 | 新增专用 helper，不修改 `default_directory_for`；用调用点测试确认范围。 |
| 未编辑判定或目录判断误把真实输入当默认值 | 只比较完整初始串；目录检查使用 expand 后的 filesystem 状态；测试编辑后回到相同文本与相似前缀场景。 |
| `write-file` 目录追加默认名绕过确认 | 追加后仍走既有 `write_file` / 覆盖确认；测试覆盖已存在文件。 |

## 实施结果与偏差

- 三条解释按计划执行：`write-file` 目录输入追加当前文件名，`find-file` 目录输入保留会话并提示，无路径 `save-buffer` 未编辑 RET 使用“默认目录 + buffer 显示名”，自动滚动只在本 view 水平范围内生效。
- 候选层的 `CandidateResolution::FileNameRequired` 删除：目录答案改由确认层按 kind 决定（`find-file` 提示、`write-file` 追加名），避免同一语义分处两层；`Completion` 单测改为断言 `Accept(input)`。
- 按下时回显 `Mark set`，与键盘设置 mark 的命令一致；计划未单列该消息。参照 dump 只在拖动按下时保留该消息，普通单击的 dump 为空，未据此宣称完全一致。
- 新增 `src/view/pointer.rs`（拖动态与自动滚动）与 `src/app/reads_tests.rs`（生成 buffer 的仓库根解析），属 `view.rs` / app 层的私有组织调整。
- 新观察的上游差异：普通单击在松开时用 `pop-mark` 恢复点击前的 mark（见 [RESULTS](../../tests/emacs/RESULTS.md#m3鼠标选区与文件读入预填参照)），本实现沿用已批准的“mark 停在点击位置”，已在保真度文档登记。
- 拖出 view 的滚动步进按“越界距离换算行、至少一行”执行，与参照的 `mouse-scroll-min-lines` / 复用首次跳数一致；滚到首尾后停止。指针移出 OS 窗口时不在 `MouseExitEvent` 处停止，而是保留最后一次窗口外位置继续滚动（与参照的窗口外继续滚动一致），松开、窗口失活或输入生命周期结束时清理。
- 计划“推荐方案”里“可见边界 offset 从 wrap index 与 scroll 计算”在执行时改为在 `prepare` 末尾用已重排的 `geometry` 反算（计划允许的执行时决定）；`MouseExitEvent` 监听因此不需要。
- 验收证据：只读文本、`*Messages*` 这类生成只读 buffer、minibuffer 共用入口（只读 / 生成 buffer 有鼠标用例，minibuffer 沿用既有 pointer 测试）；窗口失活与“移出窗口后继续滚动”有 headless 用例（`simulate_event` / `deactivate_window`）；编辑后回到初始串的未编辑判定有集成用例。Magit status 等有内容的生成 buffer 未单独加鼠标用例（headless 状态 buffer 内容为空，无可命中行）。
- 未执行：真实桌面指针单击 / 拖动 / 拖出边界 / 窗口外松开与失活的人工验收（headless 已覆盖事件序列，不代表真实平台派发顺序），候选窗与 IME 的宿主检查。
