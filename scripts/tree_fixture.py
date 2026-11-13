"""Disposable tree-viewer fixture and manual desktop observations; no Git operations."""

SEEDS = {
    "project/00-main.txt": b"F6 MAIN\nOnly inspect; do not edit or save.\n",
    "project/10-other.txt": b"F6 OTHER\n",
    "project/20-third.txt": b"F6 THIRD\n",
    "project/30-last.txt": b"F6 LAST\n",
    "project/a-dir/child.txt": b"F6 CHILD A\n",
    "project/b-dir/child.txt": b"F6 CHILD B\n",
    "project/.hidden.txt": b"F6 HIDDEN\n",
    "project/.git/marker": b"root-discovery marker only; not a Git repository\n",
    "project/.jj/marker": b"hidden directory marker\n",
    "config/semacs/init.js": b"",
}
OBSERVATIONS = ("tree-side-window", "tree-visits", "tree-locate-follow")

INSTRUCTIONS = """
F6 最小人工验证：一个窗口和它内部的 tree / 普通 pane。
本批只使用隔离 project 文件、空 JS 配置和独立 history state。
不自动注键、不改变输入法、不截图、不测性能。使用英文输入，建议窗口至少 1100 x 700。
全程不编辑、不保存，不打开日常文件，不运行 Git；.git 仅用于根识别，不是真实仓库。
任一步异常 / 未观察 / 没看清：立即停止后续操作，正常关闭窗口，回终端对相应组答 no。
不要修复 fixture、改流程或重试。其余未做组不算通过；保留整个输出目录。

1. tree-side-window：布局、保护、toggle 和宽度
   初始普通 pane 显示 00-main.txt 的 F6 MAIN。
   M-x treemacs RET：左侧出现 *Treemacs*，根行 project，约 35 列宽；焦点在树。
   a-dir / b-dir 为折叠目录；.hidden.txt 显示，.git / .jj 不显示。
   M-x treemacs-select-window RET 回到原普通 pane；再执行一次回树。
   C-x 1 应报 side window 不能成为唯一窗口，树和普通 pane 均保留。
   C-x o 到普通 pane，C-x 2 增加下方普通 pane；C-x 1 只移除另一普通 pane，树不变。
   C-x o 回树，C-x 0 关闭树侧窗；M-x treemacs RET 应恢复并选中同一树。
   M-x treemacs RET 隐藏树，再 M-x treemacs RET 显示并选中；C-x o 到普通 pane，C-x 1 仍保留树。
   M-x treemacs-select-window RET 回树，w 输入 42 RET，树应变宽；< / > 各改变一列。
   = 应收窄以适配树内容，普通 pane 仍可用；最后 w 输入 35 RET 恢复默认宽度。

2. tree-visits：访问目标、split 和前缀的真实焦点
   以下“到文件”均指树里 M-< 回根，再逐次 n 到指定文件行；勿在正文里输入这些键。
   到 00-main.txt，RET：复用原普通 pane，显示 F6 MAIN 并获得焦点；树保留，没有新 split。
   M-x treemacs-select-window RET 回树，到 10-other.txt，C-u RET：
   普通 pane 显示 F6 OTHER，但光标 / 焦点仍在树。
   到 20-third.txt，C-u C-u RET：普通 pane 显示 F6 THIRD 并获得焦点，树侧窗关闭。
   M-x treemacs RET 恢复树，到 00-main.txt，o h：右侧普通区域左右分屏，新的文件 pane 获得焦点。
   C-x 1 回到树加一个普通 pane；M-x treemacs-select-window RET 回树，到 10-other.txt，o v：
   右侧普通区域上下分屏，下方显示 F6 OTHER 并获得焦点。
   M-x treemacs-select-window RET 回树，到 20-third.txt，o r：
   在刚用过的下方 pane 显示 F6 THIRD 并获得焦点，不增加 pane。
   C-x 1 回到树加一个普通 pane；M-x treemacs-select-window RET 回树，到 30-last.txt，o c：
   选中普通 pane 的 F6 LAST 并关闭树；M-x treemacs RET 重新显示。
   到 a-dir，RET 只展开 / 收起；o h 提示不支持目录访问，不创建 split、不打开文件管理器。
   M-x treemacs-select-window RET 回到普通 pane，保持 F6 LAST。

3. tree-locate-follow：定位、空闲后 follow 和焦点
   普通 pane 执行 M-x treemacs-find-file RET：树定位 30-last.txt，焦点仍在普通 pane。
   M-x treemacs RET 隐藏树，再 M-x treemacs-find-file RET：重新显示并定位，焦点仍在普通 pane。
   C-x C-f：保留默认 project/ 前缀，在后面输入 a-dir/child.txt RET。
   短暂停顿后，树应展开 a-dir、point 到 child.txt，普通 pane 仍显示 F6 CHILD A 并保持焦点。
   C-x 2 增加下方 pane，C-x o 到下方普通 pane（若先到树，继续 C-x o，勿在树中 find-file）。
   C-x C-f：在默认 a-dir/ 后输入 ../b-dir/child.txt RET；短暂停顿后树应定位 b-dir/child.txt，焦点不动。
   C-x 0 关闭当前 B pane；回到上方 F6 CHILD A 后，树应再次定位 a-dir/child.txt，焦点仍在普通 pane。
   M-x treemacs-select-window RET 回树，t f 关闭 follow；M-x treemacs-select-window RET 回文件。
   C-x C-f：在默认 a-dir/ 后输入 ../b-dir/child.txt RET；树应保持 A，不自动移到 B。
   M-x treemacs-select-window RET 回树，t f 开启；再 select-window 回 B，短暂停顿后应自动定位 B。
   肉眼只确认“空闲后定位、没有明显长时间停滞、焦点未被抢”，不能证明精确 0.2 秒。
   最后回树按 Q 关闭并 kill 树，再在普通 pane C-x C-c 正常关闭本批窗口，不保存任何文件。

关闭窗口后回终端逐组确认 yes / no，并记录实际 / 大致尺寸、可读性及 follow 是否有明显延迟。
异常或提前关闭不算通过；驱动会独立核对全部 project / 配置路径和原始字节。
"""


def verify_paths(directory):
    """Reject changed fixture topology before the caller reads any file bytes."""
    manifest = directory / "fixture.json"
    if manifest.is_symlink() or not manifest.is_file():
        raise RuntimeError("missing or linked fixture manifest")
    expected = set(SEEDS)
    for name in SEEDS:
        parent = (directory / name).parent
        while parent != directory:
            expected.add(str(parent.relative_to(directory)))
            parent = parent.parent
    actual = set()
    for root in (directory / "project", directory / "config"):
        if root.is_symlink() or not root.is_dir():
            raise RuntimeError(f"missing or linked fixture directory: {root.name}")
        actual.add(root.name)
        for path in root.rglob("*"):
            if path.is_symlink():
                raise RuntimeError(f"linked fixture path: {path.relative_to(directory)}")
            actual.add(str(path.relative_to(directory)))
    if actual != expected:
        raise RuntimeError("tree fixture paths changed")
    return True
