#!/usr/bin/env python3
"""Two-window manual scripting smoke test with isolated, disposable configuration."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

CONFIGURATIONS = {
    "initial": b"""semacs.setFont({family: "monospace", size: 24});
semacs.registerCommand("smoke-uppercase-region", text => text.toUpperCase());
semacs.bindKey("ctrl-c u", "smoke-uppercase-region");
semacs.bindKey("ctrl-c U", "smoke-uppercase-region");
""",
    "reloaded": b"""semacs.setFont({family: "monospace", size: 32});
semacs.registerCommand("smoke-lowercase-region", text => text.toLowerCase());
semacs.bindKey("ctrl-c l", "smoke-lowercase-region");
""",
    "invalid": b"""semacs.setFont({family: "monospace", size: 40});
semacs.bindKey("ctrl-c l", null);
throw new Error("intentional scripting smoke configuration failure");
""",
}
SEEDS = {"commands.txt": b"alpha beta\n", "fallback.txt": b"fallback\n"}
OBSERVATIONS = (
    "startup-config", "command-routes", "reload-success",
    "reload-failure", "startup-failure", "fallback-editing",
)
CLOSE_TIMEOUT = 120


def digest(data):
    return hashlib.sha256(data).hexdigest()


class Session:
    def __init__(self, directory, binary, ask):
        self.directory = directory
        self.binary = binary
        self.ask = ask
        self.config_home = directory / "config"
        self.config = self.config_home / "semacs/init.js"
        self.process = None
        self.logs = []
        self.result = {
            "completed": False,
            "evidence": "user-observed desktop plus independent disk bytes",
            "performance": "unmeasured",
            "startup_visual": "user observation only; no per-frame capture",
            "ime": "unmeasured",
            "gpu": "unmeasured",
            "binary": str(binary),
            "child_environment": {"XDG_CONFIG_HOME": str(self.config_home)},
            "observations": {},
            "disk_bytes": {},
            "configurations": [],
            "processes": [],
            "errors": [],
            "stage": "prepare",
        }

    def prepare(self):
        self.config.parent.mkdir(parents=True)
        (self.directory / "files").mkdir()
        (self.directory / "configurations").mkdir()
        for name, data in SEEDS.items():
            (self.directory / "files" / name).write_bytes(data)
        for name, data in CONFIGURATIONS.items():
            (self.directory / "configurations" / f"{name}.js").write_bytes(data)
        self.config.write_bytes(CONFIGURATIONS["initial"])
        (self.directory / "fixture.json").write_text(json.dumps({
            "purpose": "disposable scripting smoke; NOT performance data",
            "seed_sha256": {name: digest(data) for name, data in SEEDS.items()},
            "configuration_sha256": {
                name: digest(data) for name, data in CONFIGURATIONS.items()
            },
        }, indent=2) + "\n")
        checksum = hashlib.sha256()
        with self.binary.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                checksum.update(chunk)
        self.result["binary_sha256"] = checksum.hexdigest()
        self.check_configuration("initial")

    def check_configuration(self, phase):
        actual = None if self.config.is_symlink() else self.config.read_bytes()
        passed = actual == CONFIGURATIONS[phase]
        self.result["configurations"].append({
            "phase": phase, "matched": passed,
            "sha256": digest(actual) if actual is not None else None,
        })
        if not passed:
            raise RuntimeError(f"fixture configuration changed unexpectedly: {phase}")

    def change_configuration(self, previous, following):
        self.check_configuration(previous)
        temporary = self.config.with_name("next-init.js")
        with temporary.open("xb") as target:
            target.write(CONFIGURATIONS[following])
        temporary.replace(self.config)
        self.check_configuration(following)

    def launch(self, name, file):
        self.result["stage"] = name
        for stream in ("stdout", "stderr"):
            self.logs.append((self.directory / f"{name}.{stream}.log").open("xb"))
        environment = os.environ.copy()
        environment["XDG_CONFIG_HOME"] = str(self.config_home)
        self.process = subprocess.Popen(
            [str(self.binary), str(self.directory / "files" / file)],
            cwd=self.directory, env=environment, stdin=subprocess.DEVNULL,
            stdout=self.logs[0], stderr=self.logs[1],
        )
        self.result["processes"].append({
            "window": name, "pid": self.process.pid, "returncode": None,
            "cleanup": False,
        })

    def require_running(self):
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("test window exited before the current step was confirmed")

    def observe(self, name, prompt):
        self.result["stage"] = name
        self.require_running()
        try:
            answer = self.ask(f"{prompt} 请输入 yes 或 no: ").strip().lower()
        except EOFError:
            self.result["observations"][name] = "unobserved"
            raise RuntimeError(f"input ended before observation: {name}") from None
        self.result["observations"][name] = answer
        self.require_running()
        if answer != "yes":
            raise RuntimeError(f"manual observation not confirmed: {name} ({answer!r})")

    def check_files(self, stage, expected):
        checks = {}
        for name, data in expected.items():
            path = self.directory / "files" / name
            actual = path.read_bytes() if path.is_file() and not path.is_symlink() else None
            checks[name] = {
                "matched": actual == data, "expected_hex": data.hex(),
                "actual_hex": actual.hex() if actual is not None else None,
            }
        self.result["disk_bytes"][stage] = checks
        failed = [name for name, check in checks.items() if not check["matched"]]
        if failed:
            raise RuntimeError(f"disk-byte mismatch at {stage}: {', '.join(failed)}")

    def close_window(self):
        print("\n请回到测试窗口按 C-x C-c 关闭；驱动自动等待，关闭前不要再编辑。", flush=True)
        code = self.process.wait(timeout=CLOSE_TIMEOUT)
        self.result["processes"][-1]["returncode"] = code
        if code != 0:
            raise RuntimeError(f"test window exited abnormally: {code}")
        self.process = None
        self.close_logs()

    def close_logs(self):
        for log in self.logs:
            log.close()
        self.logs = []

    def stop_child(self):
        try:
            if self.process is not None:
                record = self.result["processes"][-1]
                if self.process.poll() is None:
                    record["cleanup"] = True
                    self.process.terminate()
                    try:
                        self.process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        self.process.kill()
                        self.process.wait(timeout=5)
                record["returncode"] = self.process.poll()
        finally:
            self.close_logs()

    def execute(self):
        print("""
第一窗口：配置字号 24px，文本 alpha beta。只操作本批测试文件，本轮请自行使用英文/ASCII 输入。
1. 观察首次正文是否直接使用较大配置字号，未看到默认小字号再切换。
2. C-a C-t M-f 选中 alpha，C-c u 后应为 ALPHA beta；等待变换完成。
3. C-/ 撤销一次，应恢复 alpha beta。
4. 再 C-a C-t M-f；M-x smoke-uppercase-region RET，应为 ALPHA beta。
5. C-x C-s 保存，等待 clean。保持窗口打开，回终端回答下面两项。
任何异常立即回终端答 no；不要修复 fixture、继续后面的步骤或重试。
""", flush=True)
        self.launch("configured", "commands.txt")
        self.observe("startup-config", "首次正文直接使用配置字号，未观察到默认字号闪变？")
        self.observe("command-routes", "自定义键、单次 undo、M-x 变换和保存均符合上述结果？")
        self.check_files("commands", {"commands.txt": b"ALPHA beta\n", "fallback.txt": SEEDS["fallback.txt"]})

        self.change_configuration("initial", "reloaded")
        print("""
测试配置已切换到 32px / lowercase，尚未在应用中生效。
1. 回测试窗口，M-x reload-configuration RET；应显示 Configuration reloaded，字号增大到 32px。
2. C-c u 应为 undefined；M-x smoke-uppercase-region RET 应 No match，然后 C-g 取消读入。
3. C-a C-t M-f 选中 ALPHA，C-c l 应得到 alpha beta。
4. C-x C-s 保存并等待 clean。保持窗口打开，回终端确认。
""", flush=True)
        self.observe("reload-success", "重载后字号、旧命令/键位移除、新命令/键位及保存均正确？")
        self.check_files("reloaded", {"commands.txt": b"alpha beta\n", "fallback.txt": SEEDS["fallback.txt"]})

        self.change_configuration("reloaded", "invalid")
        print("""
测试配置现已故意包含错误；其中试图改成 40px 并取消 C-c l。
1. 回测试窗口，M-x reload-configuration RET；应显示以 Configuration unchanged 开头的错误（本批故意触发的 intentional...failure）。
2. 字号应保持 32px，不能变成 40px。
3. C-a，输入大写 X，应为 Xalpha beta；再 C-a C-t M-f，C-c l，应为 xalpha beta。
4. C-x C-s 保存并等待 clean。保持窗口打开，回终端确认。
""", flush=True)
        self.observe("reload-failure", "错误已报告，旧字号/脚本键位保留，原生编辑和保存仍正常？")
        self.check_files("failed-reload", {"commands.txt": b"xalpha beta\n", "fallback.txt": SEEDS["fallback.txt"]})
        self.close_window()
        self.check_files("configured-closed", {"commands.txt": b"xalpha beta\n", "fallback.txt": SEEDS["fallback.txt"]})
        self.check_configuration("invalid")

        print("""
第二窗口：沿用错误配置，打开全新的 fallback.txt。
1. 先观察：默认 16px 字号，回显以 Configuration failed; using defaults 开头的错误。
2. 确认错误后，C-a 输入大写 Z，应为 Zfallback；C-x C-s 保存并等待 clean。
3. 保持窗口打开，回终端回答下面两项；之后驱动再提示关闭。
""", flush=True)
        self.launch("fallback", "fallback.txt")
        self.observe("startup-failure", "错误配置启动已报告错误并回退默认字号，而非 24/32/40px？")
        self.observe("fallback-editing", "错误配置下原生插入和保存仍正常，正文为 Zfallback？")
        self.check_files("fallback", {"commands.txt": b"xalpha beta\n", "fallback.txt": b"Zfallback\n"})
        self.close_window()
        self.check_files("final", {"commands.txt": b"xalpha beta\n", "fallback.txt": b"Zfallback\n"})
        self.check_configuration("invalid")
        if set(self.result["observations"]) != set(OBSERVATIONS):
            raise RuntimeError("incomplete manual observation set")
        self.result["stage"] = "complete"
        self.result["completed"] = True


def run(directory, binary, ask=None):
    directory = directory.resolve()
    binary = binary.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    session = Session(directory, binary, input if ask is None else ask)
    try:
        session.prepare()
        session.execute()
    except (Exception, KeyboardInterrupt) as error:
        session.result["completed"] = False
        session.result["errors"].append(str(error) or type(error).__name__)
    finally:
        try:
            session.stop_child()
        except Exception as error:
            session.result["completed"] = False
            session.result["errors"].append(f"child cleanup failed: {error}")
    if not session.result["completed"]:
        (directory / "STOP.txt").write_text("\n".join(session.result["errors"]) + "\n")
    (directory / "result.json").write_text(json.dumps(session.result, ensure_ascii=False, indent=2) + "\n")
    return session.result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
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
    print(f"Manual observations and disk bytes verified: {args.directory / 'result.json'}")
    print("No performance, real IME or per-frame startup conclusion is implied.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
