# Emacs 对照基准

这里记录独立于 semacs 的外部行为参照，不是 semacs 的产品通过记录。
固定环境与比较要求见 [对照基准与验收](../../docs/emacs-fidelity.md#5-对照基准与验收)，实际观察见 [RESULTS.md](RESULTS.md)。

## 入口

从仓库根目录运行：

```sh
python3 tests/emacs/drive.py undo-del dirty kill2
emacs -Q --batch -l tests/emacs/export-keys.el
```

可用 case 以 `drive.py` 的 `CASES` 为准。导出绑定不等于 command loop 验证，也不代表完整键位已核对。

## 比较陷阱

- Emacs point 是 1-based 字符位置，与 UTF-8 字节位置比较须显式转换，非 ASCII 不只做减一。
- 初始文本不进入被测历史；采集不能污染 echo。一个步骤可能包含多条命令，按 dumps 范围而非步骤数比较。
- pty 连发字节可能被同一次 read 合并，改变前缀 / quit 观察；终端结果也不能证明 GUI 键名、AltGr、死键或真实 IME 行为。
- 保存点 case 中 `C-c m` 仅设置 modified，不写磁盘，不能替代产品保存安全验收。
- 真实桌面或注键测试必须先获得同意。
