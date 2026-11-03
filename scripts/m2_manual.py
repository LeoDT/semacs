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
THEMES_CONFIG = b"""semacs.setFont({family: "monospace", size: 20});
semacs.defineTheme("e0-night", {
    appearance: "dark",
    styles: {"mode-line": {foreground: "#ffffff", background: "#304060"}}
});
semacs.defineTheme("e0-paper", {
    appearance: "light",
    styles: {
        default: {foreground: "#302819", background: "#fff8e7"},
        region: {background: "#e6d4a8"},
        "mode-line": {foreground: "#fff8e7", background: "#685237"},
        "mode-line-inactive": {foreground: "#685237", background: "#e8dcc5"},
        "minibuffer-prompt": {foreground: "#7b3f00"},
        "echo-area": {foreground: "#685237", background: "#f3e8d0"},
        completion: {foreground: "#302819", background: "#fff8e7"},
        "completion-selected": {foreground: "#302819", background: "#e6d4a8"},
        "completion-match": {foreground: "#7b3f00"},
        "completion-annotation": {foreground: "#79694f"},
        separator: {foreground: "#ad926c"}
    }
});
semacs.loadTheme("e0-night");
semacs.setStyle("cursor", {foreground: "#202124", background: "#ffcc66"});
"""
THEMES_SEEDS = {
    "body.txt": "alpha beta\nsecond line: 中文 🙂 e\u0301\n".encode(),
    "control.txt": b"untouched\n",
    "config/semacs/init.js": THEMES_CONFIG,
}
THEMES_EXPECTED = {**THEMES_SEEDS, "body.txt": b"X" + THEMES_SEEDS["body.txt"]}
THEMES_OBSERVATIONS = (
    "theme-start-split", "theme-switch", "theme-cancel-unknown",
    "theme-minibuffer-messages", "theme-editing",
)
THEMES_INSTRUCTIONS = """
E0 配色效果验证（仅本批窗口，英文输入；异常立即停止步骤，关闭窗口后答 no，不修复或重试）：

1. 启动 / 分屏（theme-start-split）
   应以深色 e0-night 启动，20px 正文，黄色方块光标且覆盖文字可读。
   C-x 3 左右分屏，观察分隔线、选中与非选中 mode line；C-x o 来回切换，两侧配色一致。
   在正文 M-< C-t M-f，观察 region 背景与文字仍清楚；C-g 停用。
2. 主题切换（theme-switch）
   M-x load-theme RET：应列出 dark、e0-night、e0-paper、light，当前项注释 current。
   输入 light RET，整个窗口切成浅色；黄色光标保留，这是用户显式覆盖，不是旧主题残留。
   再 M-x load-theme RET，输入 e0-paper RET，应出现纸色正文、棕色 mode line / prompt / 候选命中。
   在 paper 中重新 M-< C-t M-f 检查 region，再 C-g；两侧及非选中 mode line 都应更新。
   再切 dark、light，确认纸色背景不残留，字号、文字、位置不因配色而改变。
3. 取消 / 不存在名称（theme-cancel-unknown）
   在 light 下再次读入主题，输入 e0-paper 后 C-g，应仍是浅色并回原正文。
   再读入主题，输入 zz-e0-missing RET，应显示 No match，保留输入，不切主题；C-g 恢复正文。
4. minibuffer / 消息（theme-minibuffer-messages）
   切 e0-paper，M-x 输入 fwd，检查 prompt、命中、注释与候选选中行可辨；C-n / C-p 移动后 C-g。
   M-x 输入 zz-e0-unknown RET，检查输入末尾 [No match]，C-g；底部不覆盖 mode line。
   C-h e 查看历史：纸色仍一致、原正文保持焦点。可缩窄测试窗口检查裁切，再恢复大小。
5. 编辑恢复（theme-editing）
   保持原正文焦点，C-x 1 只保留当前正文。M-<，输入 X，C-/ 撤销，确认原文恢复；
   再输入 X，C-x C-s 保存等待 clean，C-x C-c 正常关闭。不要改配置或 control.txt。
   回终端回答五组观察并记录大致窗口尺寸。

不会截图、注键、改变输入法或读取日常配置；不测原生性能、真实 IME 或 GPU。
"""
SYNTAX_BODY = '''// Rust syntax: 中文 🙂 é
const ANSWER: i32 = 42;
fn greet(name: &str) -> i32 {
    let text = r#"hello fn"#;
    if name.len() > 0 { ANSWER + 1 } else { 0 }
}
'''.encode()
SYNTAX_CONFIG = b'''semacs.setFont({family: "monospace", size: 20});
semacs.defineTheme("e1-custom", {
    appearance: "dark", styles: {"syntax.string": {foreground: "#ffb3d1"}}
});
semacs.loadTheme("dark");
semacs.setStyle("syntax.keyword", {foreground: "#e53935"});
semacs.setStyle("cursor", {foreground: "#202124", background: "#ffcc66"});
'''
SYNTAX_SEEDS = {
    "body.rs": SYNTAX_BODY,
    "plain.txt": SYNTAX_BODY,
    "config/semacs/init.js": SYNTAX_CONFIG,
}
SYNTAX_EXPECTED = {
    **SYNTAX_SEEDS,
    "language.txt": SYNTAX_BODY,
    "restored.rs": b"// done\n" + SYNTAX_BODY,
}
SYNTAX_OBSERVATIONS = (
    "syntax-start-split", "syntax-theme", "syntax-edit-history",
    "syntax-language", "syntax-layout-save",
)
SYNTAX_INSTRUCTIONS = """
E1 Rust 语法定向目视验证（仅本批窗口，英文输入；异常立即停止，关闭窗口后答 no，不修复或重试）：

1. 初始 / 分屏（syntax-start-split）
   body.rs 应以深色、20px 显示；等待后台高亮就绪。fn / const / let / if 等关键字为红色，
   注释、字符串、数字、类型和函数有区别；字符串内的 fn 不应被当作关键字。
   C-x 3 左右分屏，两侧语法颜色一致。M-< C-t C-n 选择首行，region 内注释颜色保留；
   C-g 后在首行用 C-f / C-b 检查黄色方块覆盖文字仍可读，不错位。
2. 主题与覆盖（syntax-theme）
   M-x load-theme RET，输入 light RET，再切 dark；关键字始终红色（用户覆盖），其它语法颜色随主题变化。
   再切 e1-custom，字符串应变粉色，关键字仍红；再切 dark，字符串应恢复内置颜色。
   两个 pane 都应更新，文字、位置与字号不因换色改变。
3. 编辑与历史（syntax-edit-history）
   M-<，连续输入 /*（不加空格），正文暂时成为未闭合块注释；等待两侧变成注释颜色。
   C-/ 一次撤销，原文及高亮恢复；C-? redo，注释颜色恢复；再 C-/ 回到原文。
   不应有旧颜色落在新文字上，也不应吞键、出现空白正文或改变另一侧 point / scroll。
4. 普通文本与另存（syntax-language）
   C-x 1 保留当前正文；C-x C-f 输入 plain.txt RET：相同文字不应有 Rust 语法颜色。
   C-x b 输入 body.rs RET 返回；C-x C-w 输入 language.txt 后 M-RET，等待 clean：颜色应变为普通文本。
   再 C-x C-w 输入 restored.rs 后 M-RET，等待 clean 和后台高亮：Rust 颜色应恢复，正文不变。
5. 布局 / 读入 / 保存（syntax-layout-save）
   将窗口缩窄（例如约 480px），M-x increase-font-size RET，检查折行和光标不因着色错位。
   M-x 输入 fn，候选、prompt 仍是读入配色，不套用 Rust 关键字颜色；C-g 返回正文。
   M-<，输入 // done 后 RET；C-x C-s 保存等待 clean，C-x C-c 正常关闭。
   不要编辑 plain.txt、body.rs 或配置；回终端确认五组观察及大致窗口尺寸。

不截图、不自动注键、不改变输入法、不读取日常配置；不测原生性能、真实 IME 或 GPU。
"""
INSTRUCTIONS = {"messages": MESSAGES_INSTRUCTIONS, "themes": THEMES_INSTRUCTIONS, "syntax": SYNTAX_INSTRUCTIONS}

PROFILES = {
    "m2": (SEEDS, EXPECTED, OBSERVATIONS),
    "candidates": (CANDIDATE_SEEDS, CANDIDATE_EXPECTED, CANDIDATE_OBSERVATIONS),
    "messages": (MESSAGES_SEEDS, MESSAGES_EXPECTED, MESSAGES_OBSERVATIONS),
    "themes": (THEMES_SEEDS, THEMES_EXPECTED, THEMES_OBSERVATIONS),
    "syntax": (SYNTAX_SEEDS, SYNTAX_EXPECTED, SYNTAX_OBSERVATIONS),
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
    if "config/semacs/init.js" in expected_files:
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
        if args.profile in INSTRUCTIONS:
            print(INSTRUCTIONS[args.profile], flush=True)
    else:
        verify(directory, args.profile)
        print(f"manual observations and disk bytes verified: {directory / 'result.json'}")


if __name__ == "__main__":
    main()
