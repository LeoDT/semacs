"""Disposable Git state for the one-window, read-only Magit acceptance profile."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def main_text(first, second):
    lines = [f"context {row:02}\n" for row in range(1, 37)]
    lines[0] = first + "\n"
    lines[27] = second + "\n"
    return "".join(lines).encode()


HEAD_FILES = {
    "00-main.txt": main_text("HEAD-OLD: first line", "HEAD-OLD: second hunk"),
    "10-long.txt": b"short HEAD line\n",
}
INDEX_FILES = {
    **HEAD_FILES,
    "00-main.txt": main_text("INDEX-NEW: first line", "INDEX-NEW: second hunk"),
}
WORKTREE_FILES = {
    "00-main.txt": main_text("WORKTREE-NEW: first line", "WORKTREE-NEW: second hunk"),
    "10-long.txt": ("LONG-BEGIN " + "0123456789-" * 800 + " LONG-END\n").encode(),
}
SEEDS = {**WORKTREE_FILES, "config/semacs/init.js": b"semacs.setFont({size: 20});\n"}
OBSERVATIONS = ("magit-display-navigation", "magit-shared-refresh", "magit-horizontal", "magit-version-visits")
INSTRUCTIONS = """
F1 只读 Magit diff：一个窗口、隔离配置、临时 Git 仓库，手动英文输入。
建议窗口至少约 1000 x 600，以便左右分屏。请记录实际或大致尺寸。
本批不自动注键、不截图、不切换输入法、不测原生性能；不要打开日常文件或配置。
任一异常立即停止，不修复数据或重试；关闭测试窗口，失败或未观察组回答 no。
本批所有磁盘正文、index、HEAD、Git 配置和 init.js 最终都应保持不变；不要执行保存或 Git 写操作。

1. 显示与导航（magit-display-navigation）
   初始 00-main.txt 首行为 WORKTREE-NEW: first line。
   M-x magit-diff-unstaged RET，等待加载。应有两条 diffstat、文件 / hunk heading，
   00-main.txt 中删除行为 -INDEX-NEW，新增行为 +WORKTREE-NEW；两处相隔较远的改动形成两个 hunk。
   文件 / hunk heading 应可辨识（默认加粗），增删颜色、当前 section 背景和光标均可读。
   M-< n n j：应从第一条 diffstat 跳到 00-main.txt 文件 heading。
   n 到首个 hunk；TAB 收起正文、留下 heading 与折叠指示，再 TAB 恢复。
   C-n 可进入 hunk 正文，p 应回该 hunk 起点；^ 应回文件 heading，n 再到首个 hunk。
   SPC / DEL 翻页应正常；最后 M-< n n j n 回首个 hunk heading，保持展开。

2. 分屏与刷新（magit-shared-refresh）
   在首个 hunk heading 按 C-x 3 左右分屏，C-x o 到另一侧。
   TAB 收起当前 hunk，两侧都应只剩 heading、隐藏相同正文。
   C-x o 回原侧，g 刷新并等待；两侧该 hunk 都应继续折叠，各自位置保持合理。
   TAB 展开，两侧正文都恢复。C-n 只移动当前侧光标，另一侧光标不跟随。
   C-x 1 只保留当前侧；不应多开 OS 窗口。

3. 长行与水平查看（magit-horizontal）
   在 unstaged diff 按 M-> C-b 到最后一条长行末尾，等待布局；应自动水平跟随并看到 LONG-END。
   该行不应软换成多行。C-a 回该行开头，应自动向左跟随并看到 +LONG-BEGIN。
   Alt+Right 向右查看，Alt+Left 向左查看；若平台保留这些组合键，用 M-x scroll-left / scroll-right RET。
   导航后光标与文字应对齐，不应丢行、留下整屏空白或无法恢复。

4. 旧侧 / index / worktree 版本（magit-version-visits）
   M-x magit-diff-staged RET。应看到 -HEAD-OLD 与 +INDEX-NEW，而不是 WORKTREE-NEW。
   M-< n n j n C-n RET：从首个 hunk 的 -HEAD-OLD 行打开旧侧快照。
   M-< 应看到 HEAD-OLD: first line；按 x 应报告只读、正文不改变。
   C-x b RET 回 staged diff（默认 other buffer），C-n 到 +INDEX-NEW 行，再 RET。
   M-< 应看到 INDEX-NEW: first line；按 x 同样应只读且不改变正文。
   C-x b RET 回 staged diff，M-x magit-diff-unstaged RET。
   M-< n n j n C-n C-n RET：从 +WORKTREE-NEW 行访问工作区。
   应回到最初的普通 00-main.txt buffer，首行仍为 WORKTREE-NEW: first line；不要输入正文。
   C-x b RET 回 diff，C-x C-c 关闭窗口，回终端确认四组结果。

若 other-buffer 默认不是预期 buffer，请在 C-x b 候选中选择本批对应的 *magit-diff staged...* buffer，
不要选择日常文件。任一步看不清或未观察到也算未通过，不以窗口正常退出代替观察。
"""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def isolated_environment():
    environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_TERMINAL_PROMPT="0", GIT_NO_LAZY_FETCH="1")
    return environment


def git(directory, *args):
    environment = isolated_environment()
    environment.update(
        GIT_AUTHOR_NAME="semacs fixture", GIT_COMMITTER_NAME="semacs fixture",
        GIT_AUTHOR_EMAIL="fixture@example.invalid", GIT_COMMITTER_EMAIL="fixture@example.invalid",
        GIT_AUTHOR_DATE="2020-01-01T00:00:00Z", GIT_COMMITTER_DATE="2020-01-01T00:00:00Z",
        LC_ALL="C",
    )
    try:
        result = subprocess.run(
            ["git", "--no-optional-locks", "--literal-pathspecs", "-c", "core.fsmonitor=false",
             "-c", f"core.hooksPath={os.devnull}", "--git-dir", str(directory / ".git"),
             "--work-tree", str(directory), *args],
            cwd=directory, env=environment, stdin=subprocess.DEVNULL,
            capture_output=True, timeout=15, check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError("fixture Git exceeded its 15 second limit") from error
    if result.returncode:
        raise RuntimeError(f"fixture Git {' '.join(args)} failed: {result.stderr.decode(errors='replace')}")
    return result.stdout


def prepare_repository(directory):
    git(directory, "init", "--quiet", "--initial-branch=f1-fixture", "--object-format=sha1", "--template=")
    for files, commit in ((HEAD_FILES, True), (INDEX_FILES, False)):
        for name, data in files.items():
            (directory / name).write_bytes(data)
        git(directory, "add", "--", *sorted(files))
        if commit:
            git(directory, "commit", "--quiet", "-m", "F1 disposable base")
    for name, data in WORKTREE_FILES.items():
        (directory / name).write_bytes(data)
    state = {
        "head": git(directory, "rev-parse", "HEAD").decode().strip(),
        "index_sha256": digest((directory / ".git/index").read_bytes()),
        "config_sha256": digest((directory / ".git/config").read_bytes()),
    }
    (directory / "git-fixture.json").write_text(json.dumps(state, indent=2) + "\n")


def verify_repository(directory):
    metadata = directory / ".git"
    if not metadata.is_dir() or metadata.is_symlink():
        raise RuntimeError("fixture .git is missing, linked or not a directory")
    for name in ("HEAD", "index", "config"):
        if not (metadata / name).is_file() or (metadata / name).is_symlink():
            raise RuntimeError(f"fixture .git/{name} is missing or linked")
    frozen = json.loads((directory / "git-fixture.json").read_text())
    checks = {
        "head_identity": frozen["head"] == git(directory, "rev-parse", "HEAD").decode().strip(),
        "index_bytes": frozen["index_sha256"] == digest((metadata / "index").read_bytes()),
        "config_bytes": frozen["config_sha256"] == digest((metadata / "config").read_bytes()),
        "tracked_paths": git(directory, "ls-files", "-z") == b"".join(name.encode() + b"\0" for name in sorted(HEAD_FILES)),
    }
    for revision, files in (("HEAD", HEAD_FILES), ("", INDEX_FILES)):
        for name, data in files.items():
            checks[f"{revision or 'index'}:{name}"] = git(directory, "cat-file", "blob", f"{revision}:{name}") == data
    return checks


def launch(directory, binary):
    environment = isolated_environment()
    environment["XDG_CONFIG_HOME"] = str(directory / "config")
    return subprocess.run(
        [str(binary), str(directory / "00-main.txt")], cwd=directory,
        env=environment, stdin=subprocess.DEVNULL, check=False,
    ).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--consent", action="store_true")
    args = parser.parse_args()
    if not args.consent:
        parser.error("explicit --consent is required for desktop launch")
    directory, binary = args.directory.resolve(), args.binary.resolve()
    if not all(verify_repository(directory).values()):
        raise RuntimeError("fixture Git state changed before launch")
    if any((directory / name).is_symlink() or (directory / name).read_bytes() != data
           for name, data in SEEDS.items()):
        raise RuntimeError("fixture file/configuration changed before launch")
    return launch(directory, binary)


if __name__ == "__main__":
    sys.exit(main())
