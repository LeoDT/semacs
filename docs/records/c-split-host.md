# 阶段 C：分屏与真实 IME 定向宿主验收

## 源码与产物身份

- 用户在正常 fish / Wayland 宿主终端执行 `split-ime`，回传批次目录 `.pi/tmp/c-split-ime-01`；本次为离线核对，助手没有再启动窗口或注入输入。
- 基于 `d7b0209775855785dbfe8acfc535deb2e78784e5` 的未提交工作树；捕获的 `source.patch` 在核对时与当前工作树一致，SHA-256 为 `19549277dfae8f30cc06a7c664dc78a5daacb4782b0f719ec93aa14c88ea68f4`。此后仅补充验收文档。
- production `semacs` SHA-256：`1839171fd3c46718e67145c28bb5c49a884e66daa7db2e5f816d44b3bd86e63e`。
- 实际运行的 `native_split_ime` SHA-256：`0f9e32d89024a200453e9a458c3e11c84cfc8a67f2f3fde98c708b19fa4dddf9`；与本地已检查的 probe 身份一致。probe 使用正式 AppShell，仅在外围增加操作提示。
- 宿主 Linux x86_64、Arch 内核 7.2.11-arch1-1，rustc 1.99.1；Wayland / Hyprland，`wayland-1`，`LANG=en_US.UTF-8`。
- 渲染器报告 `is_software_emulated=false`，设备名 `AMD Ryzen 7 7700 8-Core Processor (RADV RAPHAEL_MENDOCINO)`，驱动 radv / Mesa 26.2.5-arch1.1。这里只核对硬件路径，不据此产生性能通过结论。
- 六张 monitor 截图均为 3440×1440；实际 probe 受合成器平铺管理，不把请求的 960×640 当作实测窗口尺寸。fontconfig 的 monospace 匹配为 Noto Sans CJK SC Regular，不作为每个字形实际字体命中的证明。

## 自动结果核对

`COMPLETED.txt`、`result.json`、18 阶段 CSV 与完整 ARMED / DONE / READY 握手一致；无 STOP，stderr 为空。重新运行离线报告验证并独立读取保存文件，结果如下：

- 右正文：预编辑、Escape 取消、再次预编辑、提交“你”均通过；非目标无 marked，文本只提交一次。提交后左右 point 分别为 0 / 3 字节。
- 左正文：同样的取消 / 再提交通过；文本共享，提交后左右 point 为 3 / 6 字节。
- minibuffer：预编辑 / 取消 / 再提交通过，内容为“你”；正文保持不变。结束后恢复原正文 view 与 point，随后保存成功。
- 最终独立磁盘字节精确为 UTF-8 的 `你你alpha\nsecond\n`，SHA-256 为 `fb46eb35fd50aa21c9e26897912abe05d6c66bacd697ba13ded1c6b550c486b6`。
- driver 的原窗口焦点恢复检查通过；脚本没有全局注键或代改输入法状态。

准备分屏、选择 view、取消 minibuffer 和保存使用正式命令路由的合成 GPUI 键；拼音、取消和选择“你”提交由用户通过真实输入法完成。不能将其表述为所有操作均走过 OS 键盘。

## 六张截图的离线审查

助手检查了右正文、左正文、minibuffer 各两张预编辑截图，并查看目标区域原尺寸裁剪：

- 三个目标均能看到对应的 `ni` 预编辑及含“你”的候选窗；候选窗位于当前预编辑附近，没有沿用另一正文 view 的坐标。
- minibuffer 候选窗对齐 prompt 后的输入位置，不落在 prompt 起点；本批截图中候选行完整可见，未被窗口边缘或 monitor 下缘裁掉。
- 左右分隔线及选中 / 非选中 mode line 区分可见，与当前输入目标一致。
- 两次预编辑截图与报告中的取消 / 再次预编辑阶段相互对应；截图不单独证明取消、提交或保存，相关结果由自动断言及磁盘字节核对支持。

仅确认本批字体、实际平铺布局和 monitor 条件，不外推至其他 DPI、候选窗皮肤、多 monitor 或全部窗口尺寸。用户反馈本次操作顺畅，认可减少人工功能清单的测试方式；此反馈不额外扩大通过范围。

## 结论与未测边界

**本轮左右正文及 minibuffer 的真实 IME 定向集成通过**，上述截图范围的候选位置未见异常，无需用户重复这组功能步骤。

本批 `performance=unmeasured`：未测原生输入时延、GPU 执行 / 实际呈现、长时间持续输入或多 view 大文件性能；也未覆盖上下 / 嵌套 split 下的真实 IME 全矩阵。此前自动功能回归和 headless 基线见[自动化记录](c-automation-local.md)，剩余工作统一见[实施计划](../plan.md)。
