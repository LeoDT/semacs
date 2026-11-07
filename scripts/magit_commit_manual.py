#!/usr/bin/env python3
"""One-window F3 manual check using disposable Git state and message checkpoints."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from magit_fixture import digest, git, isolated_environment

TARGET = "00-main.txt"
HEAD_FILES = {TARGET: b"base main\n", "10-staged.txt": b"base staged\n", "20-control.txt": b"control\n"}
INDEX_FILES = {**HEAD_FILES, "10-staged.txt": b"staged change\n"}
WORKTREE_FILES = {**INDEX_FILES, TARGET: b"worktree main\n", "30-untracked.txt": b"untracked\n"}
CANCEL_SUBJECT = "F3 host canceled"
CLOSE_SUBJECT = "F3 host close declined"
SAVE_SUBJECT = "F3 host saved"
CLOSE_TIMEOUT = 120
INTRO = """F3 最小人工验证：始终一个窗口、空的独立配置、可丢弃 Git 仓库。
只用英文输入；不自动注键、不切换输入法、不截图、不测性能 / GPU / IME。
建议窗口至少约 1000 x 650，使下方临时分屏可见；请记录大致尺寸。
仅操作本批窗口，不打开日常文件，不自行运行 Git，不改配置或启用签名。
每组后回终端确认，独立 Git / 字节核对通过才给下一组。任一异常、没看清或未观察答 no，
停止后续动作，不修复 fixture、不重试。取消提交仍会保存消息；这不是回滚。
"""
STEPS = (
    ("temporary-windows", "base", """
1. 临时显示与局部 q
   初始文件 00-main.txt 为 worktree main；不要编辑或保存它。
   M-x magit-status RET：应下方分屏显示 status，上方仍是原文件，焦点在 status。
   应有 Unstaged (1): 00-main.txt、Staged (1): 10-staged.txt、Untracked (1): 30-untracked.txt。
   按 q：应收起临时 status 并回原文件，光标 / 文字可读。
   M-x magit-diff-unstaged RET：下方 diff 应有 -base main / +worktree main，焦点在 diff。
   按 q 应回原文件。再次 M-x magit-status RET，保持 status 打开供下一组使用。
   没有额外 OS 窗口、重叠、空白正文或焦点丢失；若空间不足未分屏，本批答 no，不改流程重试。
"""),
    ("cancel-commit", "canceled", f"""
2. 消息 / companion / 进程焦点与取消
   从 status 执行 M-x magit-commit-create RET，等待消息编辑与 staged companion diff。
   companion 应显示 10-staged.txt 的 -base staged / +staged change，不包含 unstaged 的 main。
   焦点应留在可编辑 git-commit 消息，而不是 diff；按 M-<，输入以下一行，再按两次 RET：
   {CANCEL_SUBJECT}
   M-x magit-process-buffer RET 应显示本次 running git commit；按 q 回消息，草稿仍在且可编辑。
   C-c C-k 取消；应收起本次编辑显示并回到 status，仍有 Staged (1) / Unstaged (1)。
   M-x magit-process-buffer RET，等待本次变为失败终态（Git 因 editor 取消退出，非 running）；
   按 q 回 status。取消是预期结果，不当作测试失败；不要再次提交或恢复草稿。
"""),
    ("decline-os-close", "active", f"""
3. 活动提交中拒绝 OS 关闭并继续编辑
   M-x magit-commit-create RET，等待新的消息编辑与 companion，确认焦点在消息。
   M-<，输入以下一行，再按两次 RET；此时不要保存或结束消息：
   {CLOSE_SUBJECT}
   用窗口管理器关闭按钮 / 日常关闭手势请求关闭这个测试窗口（不是 C-x C-c）。
   应先询问 Save file ...?，答 n；再出现 Modified buffers exist; exit anyway?，输入 yes RET；
   随后 Active Git processes exist; kill them and exit anyway?，输入 no RET。
   窗口不能关闭，提交不能被终止；应回到消息且草稿仍为上面这行。
   在该行末尾追加 !（M-< C-e 后输入 !），确认光标和编辑可用；不要保存，保持消息打开。
"""),
    ("save-commit", "committed", f"""
4. 保存完成、真实终态与刷新
   当前仍是刚才的消息。M-< C-k 删除首行文字（保留换行），输入以下一行：
   {SAVE_SUBJECT}
   C-c C-c；应收起消息及其 companion 并返回 status。
   等待 Git 真实完成和 status 自动刷新：Staged (0)、Unstaged (1): 00-main.txt、Untracked (1)。
   M-x magit-process-buffer RET 应保留之前取消的失败记录，并新增成功终态 exit status: 0，
   输出含本次提交主题；按 q 回 status。焦点 / point / mode line 可读，不应额外开窗口。
   本组 yes 后驱动独立核对提交 parent、完整 tree / index、消息和所有 worktree 字节。
"""),
)


def entries(files, tree=False):
    result = bytearray()
    for name, data in sorted(files.items()):
        oid = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
        result.extend(f"100644 {'blob ' if tree else ''}{oid}{'' if tree else ' 0'}\t{name}\0".encode())
    return bytes(result)


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    repo = directory / "repo"
    repo.mkdir()
    config = directory / "config/semacs/init.js"
    config.parent.mkdir(parents=True)
    config.write_bytes(b"")
    (directory / "runtime").mkdir()
    git(repo, "init", "--quiet", "--template=", "--initial-branch=f3-fixture", "--object-format=sha1")
    for name, value in (("user.name", "semacs fixture"), ("user.email", "fixture@example.invalid"),
                        ("core.autocrlf", "false"), ("core.filemode", "false"),
                        ("core.excludesFile", os.devnull), ("core.attributesFile", os.devnull),
                        ("core.untrackedCache", "false"), ("core.splitIndex", "false"),
                        ("commit.gpgsign", "false"), ("commit.cleanup", "strip")):
        git(repo, "config", name, value)
    for files, commit in ((HEAD_FILES, True), (INDEX_FILES, False)):
        for name, data in files.items():
            (repo / name).write_bytes(data)
        git(repo, "add", "--", *sorted(files))
        if commit:
            git(repo, "commit", "--quiet", "-m", "F3 disposable base")
    for name, data in WORKTREE_FILES.items():
        (repo / name).write_bytes(data)
    frozen = {"purpose": "one-window F3 manual functionality; NOT performance data",
              "head": git(repo, "rev-parse", "HEAD").decode().strip(),
              "HEAD_sha256": digest((repo / ".git/HEAD").read_bytes()),
              "config_sha256": digest((repo / ".git/config").read_bytes()),
              "worktree_sha256": {name: digest(data) for name, data in WORKTREE_FILES.items()}}
    encoded = (json.dumps(frozen, indent=2) + "\n").encode()
    (directory / "fixture.json").write_bytes(encoded)
    (directory / "instructions.txt").write_text(INTRO + "".join(step[2] for step in STEPS))
    return digest(encoded)


def verify(directory, phase, fixture_digest):
    if phase not in ("base", "canceled", "active", "committed"):
        raise ValueError("unknown commit checkpoint")
    encoded = (directory / "fixture.json").read_bytes()
    if digest(encoded) != fixture_digest:
        raise RuntimeError("fixture manifest changed")
    frozen = json.loads(encoded)
    repo = directory / "repo"
    for path in (repo, repo / ".git", repo / ".git/HEAD", repo / ".git/config", repo / ".git/index",
                 directory / "config", directory / "config/semacs", directory / "runtime"):
        if path.is_symlink() or not path.exists():
            raise RuntimeError(f"missing or linked fixture path: {path}")
    actual = {}
    for path in repo.rglob("*"):
        if path.is_symlink():
            raise RuntimeError(f"linked fixture path: {path}")
        relative = path.relative_to(repo)
        if relative.parts[0] != ".git" and path.is_file():
            actual[str(relative)] = path.read_bytes()
    if (repo / ".git/index.lock").exists():
        raise RuntimeError("unexpected index.lock (preserved)")
    config = directory / "config/semacs/init.js"
    if config.is_symlink() or not config.is_file() or config.read_bytes() != b"":
        raise RuntimeError("isolated configuration changed")
    if actual != WORKTREE_FILES:
        raise RuntimeError("worktree bytes / paths changed")
    if digest((repo / ".git/config").read_bytes()) != frozen["config_sha256"] or \
            digest((repo / ".git/HEAD").read_bytes()) != frozen["HEAD_sha256"]:
        raise RuntimeError("Git configuration / symbolic HEAD changed")
    index = git(repo, "ls-files", "--stage", "-z")
    if index != entries(INDEX_FILES):
        raise RuntimeError("complete index mismatch")
    head = git(repo, "rev-parse", "HEAD").decode().strip()
    refs = git(repo, "for-each-ref", "--format=%(refname) %(objectname)")
    if refs != f"refs/heads/f3-fixture {head}\n".encode():
        raise RuntimeError("unexpected refs")
    expected_files = INDEX_FILES if phase == "committed" else HEAD_FILES
    if git(repo, "ls-tree", "-r", "-z", "HEAD") != entries(expected_files, tree=True):
        raise RuntimeError("complete HEAD tree mismatch")
    if phase == "committed":
        headers, message = git(repo, "cat-file", "commit", head).split(b"\n\n", 1)
        parents = [line for line in headers.splitlines() if line.startswith(b"parent ")]
        if head == frozen["head"] or parents != [f"parent {frozen['head']}".encode()] or \
                message != (SAVE_SUBJECT + "\n").encode():
            raise RuntimeError("commit identity / parent / message mismatch")
    elif head != frozen["head"]:
        raise RuntimeError("HEAD changed before successful commit")
    message_digest = None
    if phase != "base":
        raw = (repo / ".git/COMMIT_EDITMSG").read_bytes()
        body = b"\n".join(line for line in raw.splitlines() if not line.startswith(b"#")).strip()
        expected = {"canceled": CANCEL_SUBJECT, "active": "", "committed": SAVE_SUBJECT}[phase]
        if body != expected.encode():
            raise RuntimeError("Git editor message checkpoint mismatch")
        message_digest = digest(raw)
    bridges = [path for path in (directory / "runtime").iterdir() if path.name.startswith(".semacs-editor-")]
    if len(bridges) != int(phase == "active") or any(
            not path.is_dir() or path.is_symlink() or not path.name.startswith(".semacs-editor-")
            for path in bridges):
        raise RuntimeError("unexpected editor bridge lifecycle state")
    return {"phase": phase, "head": head, "complete_index": True, "complete_tree": True,
            "worktree_configuration": True, "message_sha256": message_digest,
            "index_entries_sha256": digest(index), "editor_bridges": [path.name for path in bridges]}


class Session:
    def __init__(self, directory, binary, ask):
        self.directory, self.binary, self.ask = directory, binary, ask
        self.process = None
        self.logs = []
        self.result = {"completed": False, "stage": "prepare", "errors": [], "observations": {},
                       "checkpoints": [], "performance": "unmeasured", "ime": "unmeasured",
                       "gpu": "unmeasured", "presentation": "unmeasured", "screenshots": False,
                       "key_injection": False, "forced_cleanup": False,
                       "evidence": "user-observed desktop plus independent Git and message bytes"}

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

    def execute(self):
        self.result.update(fixture_sha256=prepare(self.directory), binary_sha256=digest(self.binary.read_bytes()),
                           binary=str(self.binary))
        self.check("before-launch", "base")
        environment = isolated_environment()
        environment.update(XDG_CONFIG_HOME=str(self.directory / "config"), TMPDIR=str(self.directory / "runtime"), LC_ALL="C")
        self.result["child_environment"] = {key: environment[key] for key in
                                            ("XDG_CONFIG_HOME", "TMPDIR", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "LC_ALL")}
        print(INTRO, flush=True)
        for stream in ("stdout", "stderr"):
            self.logs.append((self.directory / f"window.{stream}.log").open("xb"))
        self.process = subprocess.Popen([str(self.binary), str(self.directory / "repo" / TARGET)],
                                        cwd=self.directory / "repo", env=environment, stdin=subprocess.DEVNULL,
                                        stdout=self.logs[0], stderr=self.logs[1])
        self.result["pid"] = self.process.pid
        for name, phase, instructions in STEPS:
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
            self.check(name, phase)
        self.result["window_notes"] = self.ask("请记录实际 / 大致窗口尺寸与消息 / diff / 进程文字和光标可读性: ")
        self.result["stage"] = "close"
        self.save()
        print("请回 status，C-x 1 后 C-x C-c 正常关闭，不再编辑；驱动等待最多 120 秒。", flush=True)
        code = self.process.wait(timeout=CLOSE_TIMEOUT)
        if code != 0:
            raise RuntimeError(f"test window exited abnormally: {code}")
        self.check("after-close", "committed")
        self.result.update(completed=True, stage="complete")

    def cleanup(self):
        try:
            if self.process is not None and self.process.poll() is None:
                print("本批停止，不修复或重试。请取消仍活动的消息编辑，再正常关闭测试窗口；等待最多 120 秒。", flush=True)
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
    print(f"User observations and independent Git / message checkpoints verified: {args.directory / 'result.json'}")
    print("Native performance, IME, GPU / presentation remain unmeasured.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
