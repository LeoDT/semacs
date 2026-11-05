# 首次命令准备 prepaint：静态定向排查

## 范围

用户确认只检查 30 多 ms 峰值是否存在明显拖慢嫌疑，不追逐极限优化。本轮只读取既有三轮数据、产品代码和 Cargo.lock 对应依赖源码；没有运行新窗口、系统字体探针、用户配置或性能矩阵，没有修改产品 / 测量代码。HEAD 仍为 `d6a7ba4`，F1 及工具处于既有未提交状态。

实测值、宿主身份及预算结论继续以[三轮完整诊断](f1-diff-diagnostic-host-01.md)为准：首次 command-prepare draw 35.408–37.827 ms，prepaint 34.355–36.640 ms；第二次命令准备 draw 1.184–1.213 ms。本轮没有新增计时证据，不将静态嫌疑当作已证实根因。

## 最明显的嫌疑：首次粗体候选触发字体匹配冷路径

调用线索：

1. `examples/support/diff_workload.rs::Workload::prepare` 同步派发 M-x 和完整命令名，下一帧才显示候选；确认执行在后续 `begin`，慢帧尚未进入 Git diff。
2. `src/minibuffer.rs::Minibuffer::render` 使用配置字体 Lilex，给匹配部分添加 `FontWeight::BOLD`。完整命令名精确匹配时，其字符进入粗体高亮；普通源正文此前不需要这个粗体候选布局。
3. 锁定版本 `gpui-pre-wgpu 0.3.10` 的 `cosmic_text_system.rs::layout_line_no_separators` 调用 cosmic-text Advanced shaping。
4. `cosmic-text 0.19.0` 的 `FontSystem::get_font_matches` 对尚未缓存的 family / stretch / style / weight 组合，**先遍历整个字体数据库构造并排序候选，再查询目标 family 并将其移到首位**。因此即便随包 Lilex 已经覆盖全部 ASCII，也不能省掉这次全库匹配。
5. `FontMatchKey::new` 对字重不同的 face 调用 `db.with_face_data`，解析字体以检查 `wght` 可变轴。`fontdb 0.23.0` 的 `Source::File::with_data` 会打开文件并 mmap（未启用 memmap 则读文件）；Binary / SharedFile 则直接访问已有数据。实际成本取决于宿主数据库和 face source，不能据源码断言本次打开了多少文件。
6. 匹配缓存键包含字重，普通字体的已有匹配不能代替粗体匹配；同进程第二次粗体候选可以复用匹配缓存。这与“第一次约 35–38 ms，第二次约 1.2 ms”的形态一致，但尚无慢帧内部采样证明所占时长。

这比“命令字符串较长”或“diff 解析慢”更值得怀疑：可见文本很短，但首次匹配工作的规模可能由系统字体库决定。它是依赖的冷路径，不是已经确认的 semacs 算法缺陷；不建议仅凭此移除粗体、盲目预热或修改依赖。

## prepaint 的计时边界

`gpui-pre 0.3.10/src/window.rs` 在根元素 `request_layout` 返回后开始 prepaint 计时，随后才执行 `prepaint_as_root`；后者包含实际 layout 计算。`elements/text.rs::TextLayout::layout` 注册的 measured-layout 回调在计算布局时调用 `shape_text`。

因此报告中的 prepaint 不仅是 `src/view.rs::TextCanvas::prepaint -> BufferView::prepare`，也可能包括候选列表的文字测量 / shaping。不能将 34–37 ms 全归给正文准备函数，也不能仅在该函数加计时就排除字体候选路径。

## 未发现的明显问题与结论

- 候选绘制通过 `visible_range(rows.min(8))` 限定最多 8 项，不是将所有命令逐个布局；`CandidateList` 的列表内容由 Arc 共享，发布时 clone 不是深拷贝全部候选。
- 候选筛选发生在输入 / 通知刷新路径，有 snapshot 去重；没有证据把它直接归入此次 draw prepaint 峰值。自动流程同步输入多个字符的成本也不能当作真实逐键输入时延。
- 普通正文和 minibuffer 的可见 shaping / 索引仍有缓存，当前短源 buffer 场景未见扫描大 diff 或无界重复布局的明显结构性问题。

**达到本轮“找明显嫌疑”的范围：首要嫌疑是首次粗体候选引发的字体匹配全库冷路径；精确根因和占比未证实。** 本轮不做优化、不扩测，不改预算或宣称峰值已修复。若后续确实影响使用，再取得用户同意，优先对命令准备帧的布局 / 文本后端路径做一次最小定向采样，而非重跑整个 diff 矩阵。

## 用户确认与收尾

静态排查后，用户确认当前表现可接受，要求记录供以后性能测试参考，并对 F1 收尾提交。本峰值保留为非阻塞观察，不再作为本阶段必修项；没有修改产品、预热字体或追加测量。

如果嫌疑成立，同进程此前已显示相同字体族 / 字重 / 样式的文本（例如 Markdown 标题 / 粗体，或 M-x 匹配高亮），可能已建立对应匹配缓存；普通纯文本 / 代码浏览并不保证使用粗体，其他字体的粗体和新进程也不能保证复用。因此不能承诺正常编辑后一定不会出现峰值，更不能据此确认根因。

后续测试记录新进程与既往字体使用 / 显式预热，区分首次样式使用与缓存复用；现有 prepaint 还包含布局测量，定位时不能只观察正文 prepare。统一测量原则见[性能文档](../performance.md#适用原则)。既有三次超限和诊断 `acceptance=false; budget_passed=false` 保持，不改写为已修复或总体预算通过。

收尾提交前重新运行本地检查：默认 / native-profiler 全目标 Rust **560 / 631 项**、两套全目标 Clippy、Python 驱动 **103 项**与 fixture 工具 **64 项**、fmt / diff / fish、JS 语法及 TypeScript 类型检查均通过，无忽略。日志为 `.pi/tmp/f1-closeout-*.log`；没有重跑性能矩阵、原生窗口或追加 reviewer，本次收尾仅补充文档，产品 / 测量代码沿用已审查及验证版本。
