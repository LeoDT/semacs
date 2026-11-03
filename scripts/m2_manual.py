#!/usr/bin/env python3
"""Disposable fixtures and independent byte checks for manual Wayland acceptance."""

import argparse
import hashlib
import json
from pathlib import Path

OBSERVATIONS = (
    "keyboard-file-loop", "body-ime", "minibuffer-ime",
    "os-close-cancel", "os-close-clean",
)
SEEDS = {
    "alpha.txt": b"\xef\xbb\xbfalpha\r\nsecond",
    "beta.txt": b"beta\n",
    "overwrite.txt": b"outside\n",
    "close.txt": b"close\n",
}
EXPECTED = {
    "alpha.txt": b"\xef\xbb\xbfX" + "你".encode() + b"alpha\r\nsecond",
    "beta.txt": b"Ybeta\n",
    "overwrite.txt": b"\xef\xbb\xbfX" + "你".encode() + b"alpha\r\nsecond",
    "close.txt": b"Zclose\n",
}


CANDIDATE_SEEDS = {
    "source.txt": b"alpha second\nnext line\n",
    "alpha-existing.txt": b"outside\n",
    "overwrite.txt": b"outside\n",
    "sub/child-document.txt": b"child\n",
    **{
        f"sub/long-candidate-{index:02}-" + "abcdefghijklmnopqrstuvwxyz-" * 3 + "name.txt": b"long\n"
        for index in range(12)
    },
}
CANDIDATE_EXPECTED = {
    **CANDIDATE_SEEDS,
    "alpha": b"NEW-A!",
    "al": b"NEW-B",
    "overwrite.txt": "你Xalpha second\nnext line\n".encode(),
}
CANDIDATE_OBSERVATIONS = (
    "candidate-command", "candidate-files", "candidate-buffers",
    "candidate-layout-font", "candidate-minibuffer-ime", "candidate-body-ime",
)
MESSAGE_LONG = "D1-LONG-BEGIN " + "longword-" * 70 + " D1-LONG-END"
MESSAGE_MULTILINE = "D1-MULTI-BEGIN\n" + "\n".join(
    f"detail-{index:02}: intentional multiline message" for index in range(1, 13)
) + "\nD1-MULTI-END"
MESSAGES_CONFIG = f"""semacs.setFont({{family: "monospace", size: 24}});
semacs.registerCommand("d1-long-error", text => {{ throw new Error({json.dumps(MESSAGE_LONG)}); }});
semacs.registerCommand("d1-multiline-error", text => {{ throw new Error({json.dumps(MESSAGE_MULTILINE)}); }});
semacs.bindKey("ctrl-c l", "d1-long-error");
semacs.bindKey("ctrl-c m", "d1-multiline-error");
semacs.bindKey("ctrl-c b", "increase-font-size");
semacs.bindKey("ctrl-c n", "reset-font-size");
""".encode()
MESSAGES_SEEDS = {
    "body.txt": b"alpha beta\nsecond line\n",
    "control.txt": b"untouched\n",
    "config/semacs/init.js": MESSAGES_CONFIG,
}
MESSAGES_EXPECTED = {**MESSAGES_SEEDS, "body.txt": b"Xalpha beta\nsecond line\n"}
MESSAGES_OBSERVATIONS = (
    "message-long", "message-multiline-history", "message-minibuffer",
    "message-layout-font", "message-copy-editing",
)

MESSAGES_INSTRUCTIONS = """
D1 定向目视验证（只操作本批窗口，英文/ASCII 输入；任一异常立即停止步骤，关闭窗口后答 no，不修复或重试）：

1. 长消息（message-long）
   初始正文为 alpha beta / second line，字号 24px。C-a C-t M-f 选中 alpha，C-c l 故意抛错。
   等待消息出现；长行应自动折行，底部高度受限，不覆盖 mode line，正文不应被替换。
2. 多行与历史（message-multiline-history）
   C-a C-t M-f 重新选中 alpha，C-c m 故意抛出多行错误；同样不应覆盖 mode line。
   在正常大小窗口按 C-h e：应显示 *Messages*，原正文 pane 的选中标识和焦点仍保留。
   C-x o 进入历史，M-< / C-v / M-v / M-> 回看：长消息含 D1-LONG-BEGIN / D1-LONG-END，
   多行消息含 D1-MULTI-BEGIN、detail-01 至 detail-12、D1-MULTI-END，限额内内容不丢失。
   在历史按 M-< C-t C-f M-w，复制第一个字符；C-x o 回正文，C-a C-y 应插入所复制的一个字符，
   紧接 C-/ 撤销这次 yank，正文应恢复 alpha beta / second line。C-x 1 只保留正文 pane。
3. 读入提示（message-minibuffer）
   M-x，输入 zz-d1-unknown，RET。输入行末尾应出现 [No match]，原输入保持不变，无额外 echo 空行。
   立即输入 x，应得到 zz-d1-unknownx，附注清除且按键不被吞掉。
   再 RET，保持窗口焦点等约两秒，[No match] 附注应清除；候选区自己的 No match 文字可以保留。
   C-g 取消，应回原正文，正文内容不变。
4. 窄窗口 / 大字号（message-layout-font）
   将测试窗口缩窄（例如约 480px 宽），在正文连续按 C-c b 八次，将字号从 24px 加到 32px。
   C-a C-t M-f C-c m，再做一次第 3 项的未知命令读入 / 取消；观察消息、prompt、输入和 mode line 不重叠。
   C-c n 恢复 24px，按需恢复窗口大小。记下大致正常/窄窗口尺寸。
5. 正文恢复与磁盘（message-copy-editing）
   确认第 2 项复制 / yank / undo 已成功，且当前只剩正文 pane、没有读入会话。
   C-a，输入大写 X，正文应为 Xalpha beta / second line；C-x C-s 保存并等待 clean。
   C-x C-c 正常关闭窗口，回终端回答五项观察。不要编辑 control.txt 或配置，不打开日常文件。

不会截图、自动注键或改变输入法；本批不测原生性能、真实 IME 或 GPU。
"""
PROFILES = {
    "m2": (SEEDS, EXPECTED, OBSERVATIONS),
    "candidates": (CANDIDATE_SEEDS, CANDIDATE_EXPECTED, CANDIDATE_OBSERVATIONS),
    "messages": (MESSAGES_SEEDS, MESSAGES_EXPECTED, MESSAGES_OBSERVATIONS),
}


def prepare(directory, profile="m2"):
    seeds, _, _ = PROFILES[profile]
    directory.mkdir(parents=True, exist_ok=False)
    for name, data in seeds.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    (directory / "fixture.json").write_text(json.dumps({
        "purpose": "disposable manual Wayland acceptance; NOT performance data",
        "profile": profile,
        "seed_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in seeds.items()},
    }, indent=2) + "\n")


def verify(directory, profile="m2"):
    _, expected_files, required_observations = PROFILES[profile]
    errors = []
    manifest = json.loads((directory / "fixture.json").read_text())
    if manifest.get("profile", "m2") != profile:
        errors.append("fixture profile does not match verification profile")
    checks = {}
    for name, expected in expected_files.items():
        path = directory / name
        actual = path.read_bytes() if path.is_file() and not path.is_symlink() else None
        checks[name] = actual == expected
        if not checks[name]:
            errors.append(f"disk-byte mismatch: {name}")
    observations = {}
    path = directory / "observations.tsv"
    if path.is_file():
        for line in path.read_text().splitlines():
            parts = line.split("\t")
            if len(parts) == 2:
                observations[parts[0]] = parts[1]
    for name in required_observations:
        if observations.get(name) != "yes":
            errors.append(f"manual observation not confirmed: {name}")
    result = {
        "completed": not errors, "profile": profile,
        "disk_bytes": checks, "observations": observations,
        "errors": errors, "evidence": "user-observed real desktop plus independent disk bytes",
        "performance": "unmeasured",
    }
    if profile == "messages":
        result.update(ime="unmeasured", gpu="unmeasured", screenshots=False)
    (directory / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    if errors:
        raise RuntimeError("; ".join(errors))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "verify"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("--profile", choices=tuple(PROFILES), default="m2")
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.action == "prepare":
        prepare(directory, args.profile)
        print(directory)
        if args.profile == "messages":
            print(MESSAGES_INSTRUCTIONS, flush=True)
    else:
        verify(directory, args.profile)
        print(f"manual observations and disk bytes verified: {directory / 'result.json'}")


if __name__ == "__main__":
    main()
