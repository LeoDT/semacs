# 人工文件与候选式读入验证

本文是 `manual` / `candidates` 的有效操作说明，不是历史验收记录。使用前先按 [验收主流程](../acceptance.md#宿主验证的适用条件)确认必要性，取得本批同意后从 [fish 入口](../acceptance.md#宿主-fish-入口)启动；通用安全、失败停止、IME / 截图隐私与证据边界仍见主流程。

## manual：人工文件与关闭流程

两个窗口均使用本批独立空配置和默认键位，不读取日常 `init.js`。

### 第一个窗口

脚本打印本轮文件目录，下文的路径均指该目录中的可丢弃文件。不要在自己的工作文件上操作。遇到任一步异常，记录现象并答 no，停止，不通过修改期望字节掩盖问题。

1. 初始打开 `alpha.txt`，显示两行 alpha / second，初始 clean。
2. `C-x C-f` 输入 **完整 beta.txt 路径**并 RET。正文开头输入大写 `Y`；`M-x` 输入 `save-buffer` 并 RET。应显示 Ybeta，保存后 clean。
   - 可在输入命令名前用 C-a / DEL 检查 prompt 不会被删除，随后正常输入命令名。
3. `C-x b` 输入 `alpha.txt` 并 RET，正文开头输入大写 `X`。
4. 再 `C-x b`，在 minibuffer 开启中文输入法，输入 ni，观察候选位置正常，选择“你”；确认文字只进入 minibuffer，随后 `C-g`。正文仍为 Xalpha / second，point 应恢复到 X 后，焦点在正文。关闭中文输入状态。
5. 正文开启中文输入法，ni 后选择“你”；再输入 ni，观察预编辑 / 候选，然后用输入法消费的 Escape 取消预编辑。关闭中文输入状态。正文最终应为 **X你alpha / second**，无残留 ni、不重复提交。
   - 如果候选首项不是“你”，明确选择“你”，不要把不同候选结果记为程序缺陷。
6. `C-x C-s` 保存 alpha，等待 clean；`C-x C-w` 输入 **完整 overwrite.txt 路径**并 RET。应出现覆盖查询；只按 `y`，不加 RET，即可保存。文件身份变为 overwrite.txt，焦点恢复正文；用 C-f / C-b 检查运动即可，不再插入额外正文。
7. `C-x b` 切到 beta.txt，确认仍为 Ybeta；`C-x k RET` 关闭该 clean buffer。回到 overwrite.txt 后 `M-x C-g` 应取消并回显 Quit，随后 `C-x C-c` 正常退出。

在终端分别确认键盘文件闭环、正文 IME、minibuffer IME。脚本最后检查 alpha / overwrite 的 BOM、CRLF、无末尾换行及精确文字，beta 保持 LF 与末尾换行。UI 状态和候选位置仍以你的观察为证。

### 第二个窗口

1. 初始打开 close.txt，在开头输入大写 `Z`；`M-x` 输入未完成的命令名 `for`。
2. 使用平时合成器的 **关闭窗口快捷键 / 标题栏关闭按钮**，不要用 `C-x C-c` 代替 OS 关闭。当前实现应取消读命令会话并出现保存询问，而不是立即丢数据。
3. 按 RET，不能默认同意或关闭；再 `C-g` 取消。窗口必须保留，Zclose 仍在，焦点恢复正文。用 C-f / C-b 检查即可。
4. 再触发 OS 关闭，在保存询问按单键 `n`。应继续询问是否放弃仍修改的 buffer；输入完整 `no RET`，取消退出，文字仍保留。
5. `C-x C-s`，等待 clean，然后再触发 OS 关闭，应直接退出。脚本独立检查 close.txt 为 `Zclose\n`。

若平台只能走已批准的窗口级确认例外，记录实际流程、安全默认取消与焦点情况，先回报，不直接把不符上述预期的步骤勾成通过。

## 候选式 minibuffer：定向人工验证

本项使用 `candidates` 模式及本批独立空配置，初始默认键位、字号 16px；不自动注键，不测性能。不将既有 `manual` 模式的结果当作本项通过。

脚本打印的 `files` 目录为本轮全部测试文件所在位置。窗口初始打开 `source.txt`，内容为两行 `alpha second` / `next line`，clean。只操作这些文件，不使用自己的工作文件。以下各组按顺序操作；脚本在窗口退出后逐组询问，不必在终端与窗口间来回切换。建议另开本文对照。

**任何一步异常即停止，不继续后面的组、不修复 fixture 或重试。** 记录现象，必要时自行保留截图；关闭测试窗口后对失败或未观察的组答 no。若有 dirty buffer，可在退出查询明确选择不保存以结束本轮；这些只是测试文件。脚本保留失败报告和 STOP 原因，之后回传整个输出目录。

### 1. 命令、选择与取消（candidate-command）

- 初始正文按 `C-f C-f C-t C-f`，point 在 `alp` 后，mark 在 `al` 后。`M-x` 应立即展示候选、位置 / 总数及键位注释，最多 8 行，窗口不足时可更少。
- 输入 `fwd`，观察有序字符命中高亮。用方向键及 C-n / C-p 移动，输入仍为 `fwd`，边界不循环；从首候选向上可选中输入行。不要假设完整评分顺序。
- 选择 `forward-char` 后 TAB，输入变为完整命令名但未执行。C-a / DEL 不能删掉 prompt。C-g 取消后应恢复正文焦点、point、mark / active 与原滚动。
- 正文 C-g 停用 region，M-< 回到开头。再 M-x 输入 `fwd`，选择 `forward-word` 后 RET，应退出会话并把 point 移到 `alpha` 后，正文不变。
- 再 M-x 输入 `fwd`，M-RET 应提示无匹配并保留会话，不把缩写当精确命令名；C-g 取消。

### 2. 文件、目录、新名称与覆盖（candidate-files）

- C-x C-f 无输入即列出 source 所在目录，输入行被选中。输入 `su`，选中 `sub/` 后 RET 进入，输入为 `sub/`，显示子项并预选输入行；再次 RET 不应误开首文件。
- 选择 `child-document.txt` 后 TAB，应仅填入 `sub/child-document.txt`；RET 打开。C-x b 输入 `source.txt` 后 RET 返回原 buffer。
- C-x C-f 输入 `alpha`，应匹配已有的 `alpha-existing.txt`；用 M-RET 接受原始新名称，打开空的新文件 buffer。输入 **NEW-A**，C-x C-s 保存，等待 clean；C-x b 输入 `source.txt` 后 RET。
- C-x C-f 输入 `al`，从首候选向上选择输入行后 RET，打开原始新名称 `al`，不是已有候选。输入 **NEW-B**，C-x C-s 保存；再切回 source。
- source 中 M-< 后输入大写 **X**。C-x C-w 输入 `ovr`，选择 `overwrite.txt` 后 RET，应进入覆盖查询。只按 RET 不能默认同意，随后 C-g 取消，source 仍 dirty。
- 再 C-x C-w 输入 `ovr`，选择 overwrite 后 RET，再按单键 `y`。保存后身份变为 `overwrite.txt`、clean，正文为 `Xalpha second` / `next line`。原 source 文件的磁盘内容仍不变。

### 3. buffer 默认项、注释与安全确认（candidate-buffers）

- C-x b 输入 `alpha` 后 RET，进入刚创建的文件；point 到结尾 C-e，输入 **!**，使内容为 `NEW-A!`。
- 再 C-x b，候选中其他 buffer 的路径可见；C-g 取消后仍在 alpha。C-x k 应默认选中 alpha，自身候选注释为 dirty（**）及路径；RET 后出现 dirty 确认，不能按 RET 默认关闭。C-g 取消后文字 / 焦点保留；C-x C-s 保存 alpha。
- C-x b 输入 `missing-buffer` 后 M-RET，应拒绝且留在会话，不创建新 buffer。C-g 取消。
- 无输入 C-x b，核对默认项与 prompt 的默认名称相符；RET 切换到该默认 buffer。

### 4. 字体、字号、窄窗口与长名称（candidate-layout-font）

- M-x `set-font-family` RET，输入本机已安装的一个具体字体族名后 RET（不使用 `monospace` / `sans-serif` 通用别名）。按实际显示观察字体变化；系统字体回退不算字体实际命中的证明。脚本另保存 fc-match 环境信息，但它不代表随包默认字体或 GPUI 的实际命中。
- M-x `increase-font-size` RET，重复共四次，从 16px 增至 20px。
- 正常窗口以及约 480px 宽的窄窗口中，分别检查 M-x 的候选列表。连续向下到末项、向上到输入行时，选中项始终可见、不循环；输入行、计数与列表不裁切，输入行选择标识可辨。C-g 取消。
- C-x C-f 输入 `sub/`，检查多个 long-candidate 长名称的截断和选择高亮。选择一个长名称 RET 打开，再 C-x b 输入 `overwrite.txt` 后 RET；再次 C-x b 检查长名称 buffer 的文件路径注释截断。C-g 取消。
- 保留 20px 与当前字体继续下面的 IME 检查；不要求保持窄窗口，但需记下观察时的大致窗口尺寸。

### 5. minibuffer IME（candidate-minibuffer-ime）

- C-x b 开启中文输入法，输入 ni，观察预编辑及输入法候选窗位置。编辑器的候选列表应冻结，不把未提交拼音拿去过滤；IME 自己消费的上下选择等操作应正常，不与编辑器候选混淆。
- 明确选择“你”提交，应只在 minibuffer 出现一次，提交后编辑器过滤恢复（无匹配是正常的）。关闭中文输入状态，C-g 取消；正文 / 焦点恢复且没有拼音或“你”进入正文。
- 再 C-x b 输入 ni，用输入法自己消费的 Escape 取消预编辑；应无残留拼音。关闭中文输入状态，C-g 取消读入，正文输入仍正常。

### 6. 正文 IME 与收尾（candidate-body-ime）

- C-x b 输入 `overwrite.txt` 后 RET，M-< 回到开头。开启中文输入法，ni 后明确选择“你”提交；再输入 ni，观察预编辑 / 候选窗，然后用输入法自己消费的 Escape 取消。
- 关闭中文输入状态。正文应为 **你Xalpha second** / **next line**，不重复、不残留 ni；C-x C-s 保存并等待 clean。
- 可用 M-x `reset-font-size` 恢复 16px，再 `set-font-family` 恢复 `Lilex`；这些设置仅本次运行有效。
- C-x C-c 退出。全部测试 buffer 应 clean；不要额外改字。脚本询问上面六组观察以及窗口尺寸 / 字体说明，并独立核对新文件、未覆盖的原文件和所有子目录 fixture 的字节。任何未观察项目均答 no。
