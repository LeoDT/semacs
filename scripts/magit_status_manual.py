#!/usr/bin/env python3
"""One-window F2 manual check with isolated Git and independent index checkpoints."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from magit_fixture import digest, git, isolated_environment

TARGET = "00-main.txt"
HEAD_FILES = {TARGET: b"HEAD base\n", "10-staged.txt": b"HEAD other\n", "20-control.txt": b"untouched\n"}
INDEX_FILES = {**HEAD_FILES, "10-staged.txt": b"INDEX other\n"}
WORKTREE_FILES = {**INDEX_FILES, TARGET: b"WORKTREE current\n", "30-untracked.txt": b"untracked\n"}
CLOSE_TIMEOUT = 120
STEPS = (
    ("status-display-focus", "base", """
1. Status 显示 / 导航 / 双 view 焦点
   初始打开 00-main.txt，正文应为 WORKTREE current；不要编辑或保存。
   M-x magit-status RET，等待清单；当前版默认下方临时分屏（空间不足时当前 window）。
   C-x 1 只保留 status，作为下面固定双 view 操作的起点；应依次看到：
   Untracked files (1): 30-untracked.txt
   Unstaged changes (1): 00-main.txt
   Staged changes (1): 10-staged.txt
   Unmerged paths (0)。头部含仓库路径、分支 f2-fixture 及完整 HEAD。
   检查 heading、XY / 路径、当前 section、光标及 mode line 可读，无重叠或空白正文。
   M-< n TAB：折叠 Untracked，只隐藏其文件；再 TAB 展开。
   n 到 30-untracked.txt，TAB 应无操作，不能误折叠相邻组。
   M-< n n n 到 Unstaged heading；C-x 3 左右分屏，C-x o 到右侧。
   TAB 折叠，两侧均隐藏 00-main.txt；再 TAB 展开，两侧均恢复。
   n 到右侧 00-main.txt；C-x o 到左侧，左侧 point 仍在 Unstaged heading；
   再 C-x o 回右侧，point 应仍在 00-main.txt，焦点 / 选中 mode line 对应右侧。
   保持窗口打开，回终端答 yes；出现问题不要继续下面步骤，答 no。
"""),
    ("status-stage-stale", "stage", """
2. 默认 s / stage 与 diff 过时标识（仅执行一次 s）
   回窗口，当前应为右侧 status、point 在 Unstaged 的 00-main.txt。
   C-x o 到左侧，M-x magit-diff-unstaged RET；当前版在左侧下方新建临时 diff，
   左上仍是 status、左下 diff 应显示 -HEAD base / +WORKTREE current、右侧仍是 status。
   若窗口太小未新建下方 diff，停止本批并答 no，不临时改变操作流程。
   C-x o 到右侧；确认当前 section 是 Unstaged changes 下的文件 00-main.txt，
   不是 heading、其他路径或选区；按一次 s，等待成功消息和清单刷新。
   右侧应自动变为 Unstaged changes (0)、Staged changes (2)，包含 00-main.txt / 10-staged.txt；
   Untracked (1) / Unmerged (0) 不变，右侧 status 不应带 [stale]。
   左下 diff 保留旧快照，mode line 应出现可读的 [stale]；焦点仍在右侧，不抢左下。
   没有逐次确认弹窗；不要再次按 s，不要手动刷新左侧，也不要执行 u。
   等待完成后保持窗口打开，回终端确认；驱动会立即独立核对整个 index。
"""),
    ("status-unstage-refresh", "base", """
3. 默认 u / unstage 与显式刷新（仅执行一次 u）
   回右侧 status，M-< 后逐次 n 到 Staged changes 下的 00-main.txt 文件行，
   确认不是组 heading、不是 Untracked 文件、没有活动选区；按一次 u 并等待完成。
   右侧应恢复 Unstaged changes (1): 00-main.txt、Staged changes (1): 10-staged.txt；
   Untracked (1) / Unmerged (0) 不变。00-main.txt 磁盘正文不应被恢复为 HEAD。
   左下 diff 仍带 [stale]；C-x o 到左上 status，再 C-x o 到左下 diff，
   按 g 并等待刷新，[stale] 应消失，
   diff 仍为 -HEAD base / +WORKTREE current；刷新不切到右侧。
   C-x o 回右侧，确认仍为 status、point 合理，按键 / 焦点没有串到另一侧。
   不再执行 s/u，不编辑、不保存；保持窗口打开，回终端确认。
"""),
)
INTRO = """F2 最小人工验证：一个窗口及其内部分屏，空的独立 JS 配置、可丢弃 Git 仓库。
只使用英文输入；本批不自动注键、不改变输入法、不截图、不测性能。
建议窗口约 1000 x 650 或更大，结束时记录实际 / 大致尺寸及可读性。
每组完成后回终端确认；只有该组观察和磁盘核对均通过，驱动才给下一组步骤。
任何异常、没看清或未观察都答 no；不要修复 fixture、自动重试或继续后续动作。
只操作本批窗口，不打开日常文件 / 配置，不自行运行 Git，不重复 s/u。
stage / unstage 会修改本批 index，可能丢失 index-only 版本，无自动撤销。
"""


def expected_index(phase):
    files = {**INDEX_FILES, **({TARGET: WORKTREE_FILES[TARGET]} if phase == "stage" else {})}
    result = bytearray()
    for name, data in sorted(files.items()):
        oid = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
        result.extend(f"100644 {oid} 0\t{name}\0".encode())
    return bytes(result)


def metadata(repo):
    return {"head": git(repo, "rev-parse", "HEAD").decode().strip(),
            "refs": git(repo, "for-each-ref", "--format=%(refname) %(objectname)").decode(),
            "HEAD_sha256": digest((repo / ".git/HEAD").read_bytes()),
            "config_sha256": digest((repo / ".git/config").read_bytes())}


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    repo = directory / "repo"
    repo.mkdir()
    config = directory / "config/semacs/init.js"
    config.parent.mkdir(parents=True)
    config.write_bytes(b"")
    git(repo, "init", "--quiet", "--template=", "--initial-branch=f2-fixture", "--object-format=sha1")
    for name, value in (("core.autocrlf", "false"), ("core.filemode", "false"),
                        ("core.untrackedCache", "false"), ("core.splitIndex", "false"),
                        ("core.excludesFile", os.devnull), ("core.attributesFile", os.devnull),
                        ("commit.gpgsign", "false")):
        git(repo, "config", name, value)
    for files, commit in ((HEAD_FILES, True), (INDEX_FILES, False)):
        for name, data in files.items():
            (repo / name).write_bytes(data)
        git(repo, "add", "--", *sorted(files))
        if commit:
            git(repo, "commit", "--quiet", "-m", "F2 disposable base")
    for name, data in WORKTREE_FILES.items():
        (repo / name).write_bytes(data)
    frozen = {"purpose": "one-window F2 manual check; NOT performance data", "git": metadata(repo),
              "worktree_sha256": {name: digest(data) for name, data in WORKTREE_FILES.items()},
              "index_sha256": {phase: digest(expected_index(phase)) for phase in ("base", "stage")}}
    encoded = (json.dumps(frozen, indent=2) + "\n").encode()
    (directory / "fixture.json").write_bytes(encoded)
    (directory / "instructions.txt").write_text(INTRO + "".join(step[2] for step in STEPS))
    return digest(encoded)


def verify(directory, phase, fixture_digest):
    if phase not in ("base", "stage"):
        raise ValueError("unknown index checkpoint")
    encoded = (directory / "fixture.json").read_bytes()
    if digest(encoded) != fixture_digest:
        raise RuntimeError("fixture manifest changed")
    repo = directory / "repo"
    for path in (repo, repo / ".git", repo / ".git/HEAD", repo / ".git/config", repo / ".git/index",
                 directory / "config", directory / "config/semacs"):
        if path.is_symlink() or not path.exists():
            raise RuntimeError(f"missing or linked fixture path: {path}")
    if (repo / ".git/index.lock").exists():
        raise RuntimeError("unexpected fixture index.lock (preserved)")
    actual = {}
    for path in repo.rglob("*"):
        relative = path.relative_to(repo)
        if relative.parts[0] == ".git":
            continue
        if path.is_symlink():
            raise RuntimeError(f"linked worktree path: {relative}")
        if path.is_file():
            actual[str(relative)] = path.read_bytes()
    config = directory / "config/semacs/init.js"
    if config.is_symlink() or not config.is_file() or config.read_bytes() != b"":
        raise RuntimeError("isolated configuration changed")
    if actual != WORKTREE_FILES:
        raise RuntimeError("worktree bytes / paths changed")
    if metadata(repo) != json.loads(encoded)["git"]:
        raise RuntimeError("HEAD / refs / Git configuration changed")
    index = git(repo, "ls-files", "--stage", "-z")
    if index != expected_index(phase):
        raise RuntimeError(f"complete index mismatch at {phase} checkpoint")
    return {"phase": phase, "worktree": True, "configuration": True, "HEAD_refs_config": True,
            "index_entries_sha256": digest(index), "complete_index": True}


class Session:
    def __init__(self, directory, binary, ask):
        self.directory, self.binary, self.ask = directory, binary, ask
        self.process = None
        self.logs = []
        self.result = {"completed": False, "stage": "prepare", "errors": [], "observations": {},
                       "checkpoints": [], "performance": "unmeasured", "ime": "unmeasured",
                       "gpu": "unmeasured", "presentation": "unmeasured", "screenshots": False,
                       "key_injection": False, "forced_cleanup": False,
                       "evidence": "user-observed desktop plus independent Git and disk bytes"}

    def save(self):
        (self.directory / "result.json").write_text(json.dumps(self.result, ensure_ascii=False, indent=2) + "\n")

    def check(self, name, phase):
        if self.binary.is_symlink() or digest(self.binary.read_bytes()) != self.result["binary_sha256"]:
            raise RuntimeError("test binary changed")
        self.result["checkpoints"].append({"checkpoint": name, **verify(self.directory, phase, self.result["fixture_sha256"])})
        self.save()

    def require_running(self):
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("test window exited before observation / checkpoint completed")

    def launch(self):
        environment = isolated_environment()
        environment["XDG_CONFIG_HOME"] = str(self.directory / "config")
        self.result["child_environment"] = {key: environment[key] for key in
                                            ("XDG_CONFIG_HOME", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM")}
        for stream in ("stdout", "stderr"):
            self.logs.append((self.directory / f"window.{stream}.log").open("xb"))
        repo = self.directory / "repo"
        self.process = subprocess.Popen([str(self.binary), str(repo / TARGET)], cwd=repo,
                                        env=environment, stdin=subprocess.DEVNULL,
                                        stdout=self.logs[0], stderr=self.logs[1])
        self.result["pid"] = self.process.pid
        self.save()

    def observe(self, name, instructions):
        self.result["stage"] = name
        self.save()
        print(instructions, flush=True)
        self.require_running()
        try:
            answer = self.ask(f"{name}: 本组全部观察符合？请输入 yes 或 no: ").strip().lower()
        except EOFError:
            answer = "unobserved"
        self.result["observations"][name] = answer
        self.save()
        if answer != "yes":
            raise RuntimeError(f"manual observation not confirmed: {name} ({answer!r})")
        self.require_running()

    def execute(self):
        self.result["fixture_sha256"] = prepare(self.directory)
        self.result["binary_sha256"] = digest(self.binary.read_bytes())
        self.result["binary"] = str(self.binary)
        self.check("before-launch", "base")
        print(INTRO, flush=True)
        self.launch()
        for name, phase, instructions in STEPS:
            self.observe(name, instructions)
            self.check(name, phase)
            self.require_running()
        self.result["window_notes"] = self.ask("请记录实际 / 大致窗口尺寸和文字 / 光标 / [stale] 可读性: ")
        self.result["stage"] = "close"
        self.save()
        print("\n请回右侧 status，C-x 1 后 C-x C-c 正常关闭，不再编辑；驱动等待最多 120 秒。", flush=True)
        code = self.process.wait(timeout=CLOSE_TIMEOUT)
        self.result["returncode"] = code
        if code != 0:
            raise RuntimeError(f"test window exited abnormally: {code}")
        self.check("after-close", "base")
        self.result.update(completed=True, stage="complete")

    def cleanup(self):
        try:
            if self.process is not None and self.process.poll() is None:
                print("\n本批已停止。请正常关闭测试窗口，不再执行写操作；等待最多 120 秒，不会重试。", flush=True)
                try:
                    self.process.wait(timeout=CLOSE_TIMEOUT)
                except subprocess.TimeoutExpired:
                    self.result["forced_cleanup"] = True
                    self.process.terminate()
                    try:
                        self.process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        self.process.kill()
                        self.process.wait(timeout=5)
            if self.process is not None:
                self.result["returncode"] = self.process.poll()
        finally:
            for log in self.logs:
                log.close()


def run(directory, binary, ask=None):
    if directory.exists() or directory.is_symlink():
        raise FileExistsError(f"output must not exist: {directory}")
    directory, binary = directory.resolve(), binary.resolve()
    session = Session(directory, binary, input if ask is None else ask)
    try:
        session.execute()
    except (Exception, KeyboardInterrupt) as error:
        session.result["errors"].append(str(error) or type(error).__name__)
        session.result["completed"] = False
    finally:
        try:
            session.cleanup()
        except Exception as error:
            session.result["completed"] = False
            session.result["errors"].append(f"child cleanup failed: {error}")
        if directory.is_dir():
            session.save()
            if not session.result["completed"]:
                (directory / "STOP.txt").write_text("\n".join(session.result["errors"]) + "\n")
    return session.result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="NEW output directory")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--consent", action="store_true", required=True)
    args = parser.parse_args()
    try:
        result = run(args.directory, args.binary)
    except (OSError, RuntimeError) as error:
        print(f"STOP: {error}", file=sys.stderr)
        return 1
    if not result["completed"]:
        print("STOP: " + "; ".join(result["errors"]), file=sys.stderr)
        return 1
    print(f"User observations and complete index checkpoints verified: {args.directory / 'result.json'}")
    print("Native performance, IME, GPU / presentation remain unmeasured.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
