# 阶段间小增强归档（F6 前与 F11 后）

本页保存 F6 之前与 F11 之后已交付小增强的交付摘要与当时验证边界，定稿后不追随后续代码更新。有效行为以 [Emacs 保真度](../emacs-fidelity.md) 为准，当前工作和重要未完成事项以 [实施计划](../plan.md) 为准。

## F6 之前的小增强

以下增强已交付并从旧版 `plan.md` 迁出归档：

1. **视口滚动与 recenter**：point 离开视口时按 Emacs redisplay 居中，`M->` 按 `end-of-buffer` 的 `(recenter -3)` 定位，Magit `n` / `p` 跳到放不下的 hunk 时将 hunk heading 置顶（契约见 [F2 motion 差异](../emacs-fidelity.md#已登记的-f2-motion-差异)、[独立 diff 查看](../emacs-fidelity.md#独立-diff-查看)）。
2. **鼠标滚轮与横滚联动**：按 `mwheel-scroll` / `scroll-up` / `scroll-down` 调整窗口与 point，含 Shift 水平滚动、临时截断与归零恢复；GPUI 量、整行累计与收件时间加速适配已确认（契约见 [鼠标滚轮](../emacs-fidelity.md#鼠标滚轮)）。
3. **`find-file` 候选排序与注解**：按 Vertico 的 history → length → alpha 排序（fuzzy 仅过滤）、Marginalia `left` 注解格式与持久化 `file-name-history`；mtime 不参与排序、不补 `./` / `../`（契约见 [候选式读入](../emacs-fidelity.md#候选式读入阶段-b)，本地验证见 [find-file 候选排序与注解本地验证](../records/find-file-candidates-local.md)）。
4. **`kill-current-buffer` 与全局 section 折叠**：交付 `kill-current-buffer`（`C-x C-k`，含 minibuffer 内退出会话）与 Magit `S-TAB` 全局 section 可见性（外部观察见 [Magit 全局 section cycle 行为观察](../records/magit-section-cycle-global-observation.md)）。

## F11 之后的小增强

以下两项增强在 F11 收尾后交付并已提交至 `main`：

1. **Magit status / other-window 访问、窗口复用与 buffer 轮换（`cd1e865`）**：
   - 在 `magit-diff` / `magit-status` / `magit-revision` 中补齐 `C-x 4 RET` / `C-x 4 C-m`（`magit-diff-visit-file-other-window`）与窗口复用逻辑（优先复用已显示目标的另一普通窗、来源窗或其他普通窗，唯一普通窗时自动 split）。
   - 新增 `C-x <left>` / `C-x C-<left>`、`C-x <right>` / `C-x C-<right>`（`previous-buffer` / `next-buffer`），语义对齐 GNU Emacs 31.1 `switch-to-prev-buffer` / `switch-to-next-buffer`，按当前 window 独立历史轮换、不改写全局 MRU、跳过侧窗并在 dedicated 侧窗拒绝。
   - 契约见 [v1 默认键位](../emacs-fidelity.md#4-v1-默认键位) 与 [独立 diff 查看](../emacs-fidelity.md#独立-diff-查看)。
2. **kill ring / yank 接通系统剪贴板（`e3706d1`）**：
   - 按 Emacs 31.1 默认剪贴板策略（`select-enable-clipboard = t`、`select-enable-primary = nil`、`save-interprogram-paste-before-kill = nil`）通过 GPUI 与系统 `CLIPBOARD` 双向打通：写入 kill ring 时同步主光标顶项至系统剪贴板（多光标仅主光标同步），`C-y` 优先读取非空且变化的剪贴板内容压入 kill ring，不读写 `PRIMARY` 选区。
   - 契约见 [kill ring](../emacs-fidelity.md#kill-ring)。

## 验证边界

上述 F11 后增强均以本地自动测试与单测回归为准，未另作正式宿主 GUI 验收记录、真实桌面剪贴板跨进程矩阵或性能基准测试。
