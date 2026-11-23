use super::*;
use crate::mode::ModeId;
use crate::session::ReadKind;
use crate::text::ByteOffset;

fn git_oid(repo: &Repository, rev: &str) -> String {
    String::from_utf8(repo.git(&["rev-parse", rev]))
        .unwrap()
        .trim()
        .to_owned()
}

fn keystrokes(visual: &mut VisualTestContext, shell: &Entity<AppShell>, keys: &str) {
    visual.simulate_keystrokes(keys);
    wait(visual, shell);
}

fn type_text(visual: &mut VisualTestContext, text: &str) {
    let keys = text
        .chars()
        .map(|ch| {
            if ch == ' ' {
                "space".to_owned()
            } else {
                ch.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if !keys.is_empty() {
        visual.simulate_keystrokes(&keys);
    }
}

#[gpui::test]
fn diff_menu_range_completion_switch_flip_and_fixed_oid_refresh(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "base\n");
    repo.commit();
    repo.git(&["checkout", "-b", "feature"]);
    repo.write("b", "feature-only\n");
    repo.commit();
    repo.git(&["checkout", "-"]);
    repo.write("a", "main-v1\n");
    repo.commit();
    let main_branch = String::from_utf8(repo.git(&["symbolic-ref", "--short", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let main_oid = git_oid(&repo, "HEAD");
    let feat_oid = git_oid(&repo, "feature");

    let (mut visual, shell) = open(&repo, cx);
    // Dirty the file buffer to ensure diff range never auto-saves edits.
    visual.simulate_keystrokes("x");
    visual.read(|cx| {
        assert!(shell.read(cx).active_view().read(cx).buffer().is_modified());
    });

    // Open `magit-diff-range` via M-x (from normal file buffer) and test `main..` + TAB completion.
    command(&mut visual, &shell, Command::MagitDiffRange);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::MagitDiffRange { .. })
        ));
    });
    type_text(&mut visual, &format!("{main_branch}..fe"));
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);

    let diff_view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        let buffer = diff_view.read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::MAGIT_DIFF);
        assert_eq!(
            buffer.comparison().unwrap().kind,
            DiffKind::Range {
                spec: format!("{main_branch}..feature"),
                left_oid: main_oid.clone(),
                right_oid: feat_oid.clone(),
                three_dot: false,
            }
        );
        let text = buffer.text().to_string();
        assert!(text.contains("-main-v1"));
        assert!(text.contains("+feature-only"));
    });
    // File buffer remains modified and disk file unchanged.
    assert_eq!(
        std::fs::read_to_string(repo.0.join("a")).unwrap(),
        "main-v1\n"
    );

    // `D` menu -> `r` switches `..` to `...` (merge-base -> feature: only b added).
    visual.simulate_keystrokes("D");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(&ReadKind::DiffRefreshMenu)
        );
    });
    keystrokes(&mut visual, &shell, "r");
    visual.read(|cx| {
        let buffer = diff_view.read(cx).buffer();
        assert_eq!(
            buffer.comparison().unwrap().kind,
            DiffKind::Range {
                spec: format!("{main_branch}...feature"),
                left_oid: main_oid.clone(),
                right_oid: feat_oid.clone(),
                three_dot: true,
            }
        );
        let text = buffer.text().to_string();
        assert!(!text.contains("-main-v1"));
        assert!(text.contains("+feature-only"));
    });

    // `D` menu -> `f` flips revisions (`feature...main`: only a changed).
    visual.simulate_keystrokes("D");
    keystrokes(&mut visual, &shell, "f");
    visual.read(|cx| {
        let buffer = diff_view.read(cx).buffer();
        assert_eq!(
            buffer.comparison().unwrap().kind,
            DiffKind::Range {
                spec: format!("feature...{main_branch}"),
                left_oid: feat_oid.clone(),
                right_oid: main_oid.clone(),
                three_dot: true,
            }
        );
        let text = buffer.text().to_string();
        assert!(text.contains("+main-v1"));
        assert!(!text.contains("+feature-only"));
    });

    // Advance branch ref on disk; `g` refresh stays pinned to fixed OIDs and range is not stale.
    repo.write("a", "main-v2\n");
    repo.commit();
    visual.update(|_, cx| {
        let mut buffer = diff_view.read(cx).buffer().new_view();
        buffer.mark_git_stale();
        assert!(!buffer.git_is_stale());
    });
    keystrokes(&mut visual, &shell, "g");
    visual.read(|cx| {
        let text = diff_view.read(cx).buffer().text().to_string();
        assert!(text.contains("+main-v1"));
        assert!(!text.contains("+main-v2"));
    });

    // Opening another range via `d r` reuses the range diff buffer; `main..` + RET uses omitted HEAD.
    visual.simulate_keystrokes("d");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(&ReadKind::DiffMenu)
        );
    });
    keystrokes(&mut visual, &shell, "r");
    type_text(&mut visual, "feature..");
    keystrokes(&mut visual, &shell, "enter");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &diff_view);
        let text = diff_view.read(cx).buffer().text().to_string();
        assert!(text.contains("+main-v2"));
    });

    // Invalid range preserves existing range buffer projection and comparison untouched.
    let before_text = visual.read(|cx| diff_view.read(cx).buffer().text().to_string());
    let before_comparison = visual.read(|cx| diff_view.read(cx).buffer().comparison().unwrap());
    keystrokes(&mut visual, &shell, "d r");
    type_text(&mut visual, "does-not-exist..HEAD");
    keystrokes(&mut visual, &shell, "enter");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &diff_view);
        assert_eq!(diff_view.read(cx).buffer().text().to_string(), before_text);
        assert_eq!(
            diff_view.read(cx).buffer().comparison().unwrap(),
            before_comparison
        );
        assert!(
            diff_view
                .read(cx)
                .message()
                .unwrap()
                .contains("cannot be dereferenced as a commit")
        );
    });
}

#[gpui::test]
fn failed_switch_range_type_on_unrelated_histories_preserves_old_projection(
    cx: &mut TestAppContext,
) {
    let repo = Repository::new();
    repo.write("a", "main\n");
    repo.commit();
    let main_branch = String::from_utf8(repo.git(&["symbolic-ref", "--short", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    repo.git(&["checkout", "--orphan", "orphan"]);
    repo.git(&["rm", "-rf", "."]);
    repo.write("orphan.txt", "alone\n");
    repo.commit();

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffRange);
    wait(&mut visual, &shell);
    type_text(&mut visual, &format!("{main_branch}..orphan"));
    keystrokes(&mut visual, &shell, "enter");
    let diff_view = visual.read(|cx| shell.read(cx).active_view().clone());
    let before_text = visual.read(|cx| diff_view.read(cx).buffer().text().to_string());
    let before_comparison = visual.read(|cx| diff_view.read(cx).buffer().comparison().unwrap());
    assert!(before_text.contains("+alone"));

    // Switching `..` -> `...` fails (no merge base) and keeps old projection + comparison.
    keystrokes(&mut visual, &shell, "D r");
    visual.read(|cx| {
        assert_eq!(diff_view.read(cx).buffer().text().to_string(), before_text);
        assert_eq!(
            diff_view.read(cx).buffer().comparison().unwrap(),
            before_comparison
        );
        assert!(
            diff_view
                .read(cx)
                .message()
                .unwrap()
                .contains("Diff unchanged:")
        );
    });
}

#[gpui::test]
fn log_region_selection_and_dwim_open_range_and_deactivate_mark(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "v1\n");
    repo.commit();
    let c1 = git_oid(&repo, "HEAD");
    repo.write("a", "v2\n");
    repo.commit();
    let c2 = git_oid(&repo, "HEAD");
    repo.write("a", "v3\n");
    repo.commit();
    let c3 = git_oid(&repo, "HEAD");

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitLogCurrent);
    wait(&mut visual, &shell);
    let log_view = visual.read(|cx| shell.read(cx).active_view().clone());

    // Select region from c3 (top) down to c1 (bottom) in magit-log.
    visual.update(|window, cx| {
        log_view.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let top = ByteOffset(text.find(&c3[..12]).unwrap());
            let bottom = ByteOffset(text.find(&c1[..12]).unwrap());
            view.show_location(top, window, cx);
            let mut buffer = view.buffer().new_view();
            buffer.set_mark(top).unwrap();
            view.show_location(bottom, window, cx);
        });
    });
    visual.read(|cx| {
        assert_eq!(
            log_view.read(cx).buffer().history_region_commits(),
            Some((c1.clone(), c3.clone()))
        );
        assert!(log_view.read(cx).buffer().mark_active());
    });

    // `d d` immediately opens `c1..c3` without minibuffer prompt and deactivates mark.
    keystrokes(&mut visual, &shell, "d d");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(!log_view.read(cx).buffer().mark_active());
        let active = shell.read(cx).active_view().read(cx);
        assert_eq!(active.buffer().mode(), ModeId::MAGIT_DIFF);
        assert_eq!(
            active.buffer().comparison().unwrap().kind,
            DiffKind::Range {
                spec: format!("{c1}..{c3}"),
                left_oid: c1.clone(),
                right_oid: c3.clone(),
                three_dot: false,
            }
        );
        let text = active.buffer().text().to_string();
        assert!(text.contains("-v1"));
        assert!(text.contains("+v3"));
    });

    // Quit back to log, test `d r` with multi-commit region from c3 to c2.
    visual.simulate_keystrokes("q");
    visual.read(|cx| assert_eq!(shell.read(cx).active_view(), &log_view));
    visual.update(|window, cx| {
        log_view.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let top = ByteOffset(text.find(&c3[..12]).unwrap());
            let mid = ByteOffset(text.find(&c2[..12]).unwrap());
            view.show_location(top, window, cx);
            let mut buffer = view.buffer().new_view();
            buffer.set_mark(top).unwrap();
            view.show_location(mid, window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d r");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(!log_view.read(cx).buffer().mark_active());
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("-v2"));
        assert!(text.contains("+v3"));
    });

    // Single commit DWIM (`d d` without region) on c2 opens c2^..c2 (c1..c2).
    visual.simulate_keystrokes("q");
    visual.update(|window, cx| {
        log_view.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let mid = ByteOffset(text.find(&c2[..12]).unwrap());
            view.show_location(mid, window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d d");
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("-v1"));
        assert!(text.contains("+v2"));
    });

    // Single commit DWIM on root commit c1 reports c1^ cannot be dereferenced as a commit.
    visual.simulate_keystrokes("q");
    visual.update(|window, cx| {
        log_view.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let root = ByteOffset(text.find(&c1[..12]).unwrap());
            view.show_location(root, window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d d");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &log_view);
        assert!(
            log_view
                .read(cx)
                .message()
                .unwrap()
                .contains("cannot be dereferenced as a commit")
        );
    });
}

#[gpui::test]
fn status_dwim_working_tree_and_global_show_commit_follow_contract(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "v1\n");
    repo.commit();
    let c1 = git_oid(&repo, "HEAD");
    repo.git(&["tag", "first"]);
    repo.write("a", "v2\n");
    repo.commit();
    let c2 = git_oid(&repo, "HEAD");
    repo.write("a", "staged\n");
    repo.git(&["add", "a"]);
    repo.write("a", "worktree\n");

    let (mut visual, shell) = open(&repo, cx);
    // Dirty the file buffer; global `magit-show-commit` must not auto-save it.
    visual.simulate_keystrokes("z");
    command(&mut visual, &shell, Command::MagitShowCommit);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::MagitShowCommit { .. })
        ));
    });
    type_text(&mut visual, "first");
    keystrokes(&mut visual, &shell, "enter");
    let rev_view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        let buffer = rev_view.read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::MAGIT_REVISION);
        assert!(buffer.text().to_string().contains(&c1));
    });
    assert_eq!(
        std::fs::read_to_string(repo.0.join("a")).unwrap(),
        "worktree\n"
    );

    // In `magit-revision` body (not on a Parent line), `d c` prompts to switch commits.
    keystrokes(&mut visual, &shell, "d c");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().map(|session| session.kind()),
            Some(ReadKind::MagitShowCommit { .. })
        ));
    });
    type_text(&mut visual, "HEAD");
    keystrokes(&mut visual, &shell, "enter");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &rev_view);
        assert!(rev_view.read(cx).buffer().text().to_string().contains(&c2));
    });

    // Invalid commit preserves existing revision buffer unchanged.
    keystrokes(&mut visual, &shell, "d c");
    type_text(&mut visual, "no-such-commit");
    keystrokes(&mut visual, &shell, "enter");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &rev_view);
        assert!(rev_view.read(cx).buffer().text().to_string().contains(&c2));
        assert!(
            rev_view
                .read(cx)
                .message()
                .unwrap()
                .contains("no-such-commit cannot be dereferenced as a commit")
        );
    });

    // On `Parent 1:` line in `magit-revision`, `d c` directly visits the parent without prompting.
    visual.update(|window, cx| {
        rev_view.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find("Parent 1:").unwrap();
            view.show_location(ByteOffset(offset), window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d c");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(rev_view.read(cx).buffer().text().to_string().contains(&c1));
    });

    // `d w` opens working-tree diff (HEAD -> worktree); `RET` visits worktree on `+` line.
    keystrokes(&mut visual, &shell, "d w");
    let wt_view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        let buffer = wt_view.read(cx).buffer();
        assert_eq!(buffer.comparison().unwrap().kind, DiffKind::Worktree);
        assert!(buffer.text().to_string().contains("+worktree"));
    });

    // Status `d d` on Unstaged / Staged opens the corresponding diff.
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status_view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.update(|window, cx| {
        status_view.update(cx, |view, cx| {
            let offset = view
                .buffer()
                .text()
                .to_string()
                .find("Staged changes")
                .unwrap();
            view.show_location(ByteOffset(offset), window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d d");
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            DiffKind::Staged
        );
    });
    visual.simulate_keystrokes("q");
    visual.update(|window, cx| {
        status_view.update(cx, |view, cx| {
            let offset = view
                .buffer()
                .text()
                .to_string()
                .find("Unstaged changes")
                .unwrap();
            view.show_location(ByteOffset(offset), window, cx);
        });
    });
    keystrokes(&mut visual, &shell, "d d");
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            DiffKind::Unstaged
        );
    });
}

#[gpui::test]
fn flipping_range_resets_auto_collapsed_deleted_file_and_point(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("gone.txt", "hello\n");
    repo.commit();
    let c1 = git_oid(&repo, "HEAD");
    std::fs::remove_file(repo.0.join("gone.txt")).unwrap();
    repo.git(&["rm", "gone.txt"]);
    repo.commit();
    let c2 = git_oid(&repo, "HEAD");

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffRange);
    wait(&mut visual, &shell);
    type_text(&mut visual, &format!("{c1}..{c2}"));
    keystrokes(&mut visual, &shell, "enter");
    let diff_view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        let sections = diff_view.read(cx).buffer().sections().unwrap();
        let file_node = sections
            .nodes()
            .iter()
            .find(|node| node.key == "file/\"gone.txt\"")
            .unwrap();
        assert!(file_node.collapsed, "deleted file starts collapsed");
    });
    // Move point into the diff buffer before flipping.
    visual.simulate_keystrokes("n n");
    visual.read(|cx| assert_ne!(diff_view.read(cx).buffer().point(), ByteOffset(0)));

    // Flipping c1..c2 -> c2..c1 turns gone.txt into an added file, which must start expanded
    // with point reset to 0 rather than inheriting the deleted file's collapsed state.
    keystrokes(&mut visual, &shell, "D f");
    visual.read(|cx| {
        let buffer = diff_view.read(cx).buffer();
        assert_eq!(buffer.point(), ByteOffset(0));
        let sections = buffer.sections().unwrap();
        let file_node = sections
            .nodes()
            .iter()
            .find(|node| node.key == "file/\"gone.txt\"")
            .unwrap();
        assert!(
            !file_node.collapsed,
            "added file after flip must not stay collapsed"
        );
    });
}
