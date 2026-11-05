use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output = command
        .args([
            "--no-optional-locks",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap()
}

pub fn create() -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        ".pi/tmp/diff-tool-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&fixture).unwrap();
    let fixture = fixture.canonicalize().unwrap();
    let repo = fixture.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(
        &repo,
        &["init", "--quiet", "--template=", "--object-format=sha1"],
    );
    for version in ["HEAD", "INDEX", "WORKTREE"] {
        let text: String = (0..96)
            .map(|row| {
                format!(
                    "{} line {row:03}{}\n",
                    if row == 8 || row == 40 || row == 72 {
                        version
                    } else {
                        "context"
                    },
                    if row == 72 {
                        "x".repeat(4096)
                    } else {
                        String::new()
                    }
                )
            })
            .collect();
        std::fs::write(repo.join("file-0000.txt"), text).unwrap();
        if version != "WORKTREE" {
            git(&repo, &["add", "."]);
        }
        if version == "HEAD" {
            git(&repo, &["commit", "--quiet", "-m", "test"]);
        }
    }
    for kind in ["staged", "unstaged"] {
        let mut args = vec![
            "diff",
            "--patch",
            "--no-prefix",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-indent-heuristic",
        ];
        if kind == "staged" {
            args.push("--cached");
        }
        let patch = git(&repo, &args);
        let hunks = &patch[patch.find("@@ ").unwrap()..];
        let body = format!("1 files changed\n file-0000.txt | +3 -3\nM file-0000.txt\n{hunks}");
        let headings: Vec<_> = body
            .lines()
            .enumerate()
            .filter(|(_, line)| line.starts_with("@@ "))
            .map(|(i, _)| i)
            .collect();
        std::fs::write(
            fixture.join(format!("{kind}.projection")),
            format!("{} {}\n", headings[0] + 1, headings[1]),
        )
        .unwrap();
        std::fs::write(fixture.join(format!("{kind}.expected")), body).unwrap();
    }
    fixture
}
