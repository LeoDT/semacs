# Emacs 对照基准

这里是独立于 semacs 实现的 GNU Emacs 31.1 行为参照，不是 semacs 的产品通过记录。
固定 `emacs -Q`、`fundamental-mode`、transient mark，按真实 command loop 观察文本、point、mark、active、modified 与 echo。

## 入口

从仓库根目录运行：

```sh
python3 tests/emacs/drive.py undo-del dirty kill2
emacs -Q --batch -l tests/emacs/export-keys.el
```

- `drive.py` 在私有 pty 内启动 `emacs -Q -nw -l baseline.el`，注入终端按键并采集每条命令后的状态。
- 输出写入 `.pi/tmp/emacs-reference/emacs-<case>.jsonl`，相同 case 重跑会覆盖该临时输出。
- 可用 case 见 `drive.py` 的 `CASES`；无参数不执行用例。
- `export-keys.el` 只查询键位，不驱动 command loop；其存在不代表完整键位核对已经完成。
- 已有观察与采集限制见 [RESULTS.md](RESULTS.md)，semacs 需另行走产品命令链路验收。

## 比较约定

- Emacs point 为 1-based；与 semacs 的 UTF-8 字节位置比较时显式转换，非 ASCII 不只做减一。
- 初始文本不进入被测历史；采集写文件不能污染 echo area。
- 一个步骤可包含多条命令，须按输出的 dumps 范围解释，不能只比较步骤数。
- pty 可能把连发字节合并进同一次 read；前缀 / quit 受此影响，须区分独立按键与合并输入。
- 保存点 case 中 `C-c m` 仅设置 modified 标记，不写磁盘；不能代替 semacs 文件保存安全测试。
- 终端结果不能证明 GUI 键名、死键、AltGr、鼠标或真实 IME 行为；这些应另做平台验收。
- 不自动启动用户真实桌面应用或注键；真实会话验证必须先征求同意。
