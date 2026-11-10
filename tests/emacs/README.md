# Emacs 对照基准

这里记录独立于 semacs 的外部行为参照，不是 semacs 的产品通过记录。
固定环境与比较要求见 [对照基准与验收](../../docs/emacs-fidelity.md#5-对照基准与验收)，实际观察见 [RESULTS.md](RESULTS.md)。

## 入口

从仓库根目录运行：

```sh
python3 tests/emacs/drive.py undo-del dirty kill2
python3 tests/emacs/drive.py m1-copy-kill-yank m1-readonly-kill m1-kill-copy-direction
python3 tests/emacs/drive.py m1-redo-backward-delete m1-redo-forward-delete m1-redo-after-motion m1-save-crossing m1-save-twice
python3 tests/emacs/drive.py m1-minibuffer-prompt m1-minibuffer-motion m1-minibuffer-history
python3 tests/emacs/drive.py m2-mx-complete m2-mx-exit m2-mx-unique m2-mx-unknown m2-mx-empty m2-mx-cancel
emacs -Q --batch -l tests/emacs/m2-files.el -f ert-run-tests-batch-and-exit
emacs -Q --batch -l tests/emacs/export-keys.el
emacs -Q --batch -l tests/emacs/m1-indent.el -l tests/emacs/m1-editing.el -l tests/emacs/m1-motion.el -l tests/emacs/m1-kill.el -l tests/emacs/m1-history.el -l tests/emacs/m1-fields.el -f ert-run-tests-batch-and-exit
```

可用 command-loop case 以 `drive.py` 的 `CASES` 为准。`m1-bindings.tsv` 固定 Emacs 31.1 的原始绑定查询与缩进选项，供 Rust keymap 测试对照；`m2-minibuffer-bindings.tsv` 固定 `M-x` 使用的 must-match map 绑定；两者都不证明 GUI 键名翻译或命令执行通过。`m1-indent.el` / `m1-editing.el` / `m1-motion.el` / `m1-kill.el` 固定基础缩进、编辑与 motion 结果，属于直接函数调用，不证明 command loop / 历史分组通过。motion 中显式使用逻辑行；窗口测试仅查询 batch 的 selected window，首个翻页用例不证明连续翻页或 GUI 布局一致。词边界已知差异单独断言 Emacs 的结果，产品规则见 F2 登记。

`m1-kill.el` 固定行尾 / 空白 kill、方向与 copy 命令链、空 / inactive region、只读复制副作用、yank mark 与 120 条 ring 容量。直接函数测试显式模拟 `last-command` 和 transient mark 的延迟停用，不等同真实 command loop。
`drive.py` 的三项 `m1-copy-kill-yank` / `m1-readonly-kill` / `m1-kill-copy-direction` 用例另走隔离 pty command loop；`baseline.el` 采集 UTF-8 base64 ring 条目，清空初始 ring、禁用 interprogram 剪贴板与 copy blink delay，避免采集受剪贴板和反馈等待干扰。

`m1-history.el` 固定显式 undo boundary 下的 Unicode 回放、redo point、marker 条件恢复及内部 modified 保存标记。分组仍以 pty command loop 为参照，不用显式 boundary 的直接调用证明自动分组。参照 helper 在 undo 前停用 region，以比较已批准的全局 undo；只读测试使用 `call-interactively`，因为直接调用 `undo-only` 本身绕过 interactive 的只读检查。`C-c m` 和产品 `begin_save` / `mark_saved_at` 的模型层测试都只用于保存点状态，不证明磁盘保存已实现。

`m1-fields.el` 的 7 项直接函数 / interactive ERT 用带只读属性和 field 的前缀，核对 C-a、C-e、prompt 边界编辑、只读 kill 的复制范围、跨 prompt copy、yank 在只读文字中的 mark 副作用及初始化 prompt 不进历史。它不代表实现完整 text-property 系统或真实 minibuffer 会话。尤其 C-e 并不受 field 末端约束，不能和 C-a 一概处理。

六个 `m2-mx-*` pty 用例直接驱动真实 `M-x`，固定 prompt、公共前缀补全、RET 的补全后退出 / 停留、空输入与取消回显；候选集合与 Emacs obarray 不同，只比较查询语义，不比较候选清单。本记录不证明 semacs 的 `M-x` 已通过验收。

`m2-files.el` 的 6 项 ERT 用直接函数 / interactive spec 固定文件命名、同一文件对象复用、删除后保存、`C-x C-w` 覆盖查询、`C-x k` 读名默认值及 `C-x b` 的 default / 候选排除当前 buffer。它只替换用户查询本身，不是完整 command loop 验收；退出保存的真实 pty 观察见 [RESULTS.md](RESULTS.md#m2-文件--buffer--退出参照)。

三个 `m1-minibuffer-*` pty 用例通过参照专用的 C-c t 打开 `read-from-minibuffer`；`baseline.el` 的 `prompt_end` 是 Emacs 1-based 字符位置，原始 `text` 包含 prompt，驱动额外展示输入后缀。正文与 minibuffer 的 point / mark 比较须分别换算 UTF-8 字节位置。参照使用真实读入会话的 C-g 结束；产品 M1 只挂载 / 卸载编辑组件，不据此宣称已实现读入确认 / 取消或完整 minibuffer-local-map。

## D1 消息参照

`emacs -Q --batch -l tests/emacs/d1-messages.el -f ert-run-tests-batch-and-exit` 核对 Emacs 31.1 的默认限额 / 超时、`C-h e`、单行重复折叠、多行完整日志及显示历史不主动选择窗口；键位固定于 `d1-bindings.tsv`。ERT 会动态改变 `message-log-max`，默认值在运行测试前采集。

读入注记及清除策略另按本机 31.1 的 `minibuffer.el` 中 `minibuffer-message`、`set-minibuffer-message`、`clear-minibuffer-message` 和 `minibuffer-message--message` 核对：显式提示默认两秒，普通 message 默认等下一输入，均以 overlay 保持原输入不变。这是源码参照，不证明活跃 minibuffer 的真实 command loop、GUI 像素布局或 IME 表现。

## F5：Magit 提交与历史参照

`f5-magit.el` 使用固定 Magit checkout 的真实函数及隔离 Git fixture；只显式加载列出的依赖，不加载日常 init，不执行 `package-initialize`。以本轮环境为例：

```sh
emacs -Q --batch \
  -L "$HOME/.emacs.d/elpa/compat-31.0.0.2" \
  -L "$HOME/.emacs.d/elpa/cond-let-20260817.452" \
  -L "$HOME/.emacs.d/elpa/dash-20260221.1346" \
  -L "$HOME/.emacs.d/elpa/llama-20260601.1455" \
  -L "$HOME/.emacs.d/elpa/transient-20260825.819" \
  -L "$HOME/.emacs.d/elpa/with-editor-20260731.2234" \
  -L "$HOME/ghq/github.com/magit/magit/lisp" \
  -l tests/emacs/f5-magit.el -f ert-run-tests-batch-and-exit
```

运行前核对 Magit 身份；路径或依赖不同应另记环境，不默认为同一基准。本轮固定 checkout 为 `942d61d5de37c89a3a7c6cf693696b900b1d97b0`（`v4.7.1-74-g942d61d5`）。fixture 只创建于 `.pi/tmp/`，隔离系统 / 用户 Git 配置及替代仓库环境，正常结束删除；不操作日常仓库。测试捕获部分查询、日志选择和 Git 启动器；reword 另用临时 editor 与真实 Git 检查 tree / index，root / merge revision 使用真实 batch buffer。它不证明真实 command loop、按键输入、GUI 或 semacs 通过。结果与依赖源码身份见 [F5 记录](RESULTS.md#f5magit-提交与历史参照)，静态入口见 [调研](../../docs/work/f5-history-commit.md)。

## 比较陷阱

- Emacs point 是 1-based 字符位置，与 UTF-8 字节位置比较须显式转换，非 ASCII 不只做减一。
- 初始文本不进入被测历史；采集不能污染 echo。一个步骤可能包含多条命令，按 dumps 范围而非步骤数比较。
- pty 连发字节可能被同一次 read 合并，改变前缀 / quit 观察；终端结果也不能证明 GUI 键名、AltGr、死键或真实 IME 行为。
- 保存点 case 中 `C-c m` 仅设置 modified，不写磁盘，不能替代产品保存安全验收。
- 真实桌面或注键测试必须先获得同意。
