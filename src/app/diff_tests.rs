use super::*;
use crate::diff::tests::Repository;
use crate::files::{DecodedFile, FileFormat};
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};
use std::time::{Duration, Instant};

#[path = "diff_review_tests.rs"]
mod review;
#[path = "diff_state_tests.rs"]
mod state;

fn open(repo: &Repository, cx: &mut TestAppContext) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        Some(repo.0.join("a")),
        DecodedFile {
            text: "old\n".into(),
            format: FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    (visual, shell)
}
fn command(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        })
    });
}
fn wait(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell.git_task.is_none() && shell.file_task.is_none() && shell.opening.is_none()
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "Git/file read did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[gpui::test]
fn commands_open_refresh_and_visit_index_snapshots_not_worktree(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "indexed\n");
    repo.git(&["add", "a"]);
    repo.write("a", "worktree\n");
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    let diff = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        assert_eq!(diff.read(cx).buffer().mode(), ModeId::MAGIT_DIFF);
        assert!(
            diff.read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("+indexed")
        );
    });
    visual.update(|window, cx| {
        diff.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find("+indexed").unwrap();
            view.show_location(crate::text::ByteOffset(offset + 2), window, cx);
        })
    });
    command(&mut visual, &shell, Command::MagitDiffVisit);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), "indexed\n");
        assert!(view.buffer().is_read_only());
        assert_eq!(view.buffer().point(), crate::text::ByteOffset(1));
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&diff, cx).unwrap();
            shell.activate(index, window, cx);
        })
    });
    repo.write("a", "indexed2\n");
    repo.git(&["add", "a"]);
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &diff);
        assert!(
            diff.read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("+indexed2")
        );
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
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
        )
    });
}

#[gpui::test]
fn unavailable_commands_and_contextual_mx_do_not_change_fundamental_buffers(
    cx: &mut TestAppContext,
) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::SectionToggle);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("unavailable")
        )
    });
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let candidates = shell.session.as_ref().unwrap().candidates().unwrap();
        assert!(
            !candidates
                .items
                .iter()
                .any(|item| item.candidate.label == "section-toggle")
        );
    });
}

#[gpui::test]
fn late_open_does_not_steal_focus_and_killed_refresh_cannot_publish(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("other"), window, cx);
            shell.activate(index, window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view().read(cx).buffer().base_name(), "other");
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
        );
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate(0, window, cx);
        })
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.active_index();
            shell.remove_buffer(index, window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            !shell
                .read(cx)
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
        )
    });
}

fn redraw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn section_movement_pins_a_hunk_heading_when_the_hunk_does_not_fit(cx: &mut TestAppContext) {
    let repo = Repository::new();
    let lines: Vec<String> = (1..=120).map(|line| format!("line {line}\n")).collect();
    repo.write("a", lines.concat());
    repo.commit();
    let mut changed = lines;
    changed[0] = "changed 1\n".into();
    for (line, text) in changed.iter_mut().enumerate().take(60).skip(19) {
        *text = format!("changed {}\n", line + 1);
    }
    repo.write("a", changed.concat());
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let hunks = visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        let hunks: Vec<_> = view
            .buffer()
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .filter(|node| node.hunk)
            .map(|node| node.range.start)
            .collect();
        assert_eq!(hunks.len(), 2);
        hunks
    });
    let state = |visual: &VisualTestContext| {
        visual.read(|cx| {
            let view = shell.read(cx).active_view().read(cx);
            let layout = view.viewport_layout().unwrap();
            let index = view.wrap_index().unwrap();
            let rows: Vec<_> = hunks
                .iter()
                .map(|offset| index.row_for_offset(*offset).unwrap().index)
                .collect();
            let line = view.line_height();
            let available = (layout.bounds().size.height - line).max(px(0.));
            (
                view.buffer().point(),
                layout.scroll().y,
                rows,
                line,
                available,
            )
        })
    };
    // The first hunk fits in the window, so moving to it does not scroll.
    visual.simulate_keystrokes("n n n n");
    redraw(&mut visual);
    let (point, scroll, ..) = state(&visual);
    assert_eq!(point, hunks[0]);
    assert_eq!(scroll, px(0.));
    // The second hunk starts inside the window but its body does not fit, so
    // Magit's hunk movement hook puts its heading at the top.
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    let (point, scroll, rows, line, _) = state(&visual);
    assert_eq!(point, hunks[1]);
    assert_eq!(scroll, line * rows[1] as f32);
    // Other sections follow redisplay: moving up recenters instead of pinning.
    visual.simulate_keystrokes("^");
    redraw(&mut visual);
    let (point, scroll, _, line, available) = state(&visual);
    let row = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .wrap_index()
            .unwrap()
            .row_for_offset(point)
            .unwrap()
            .index
    });
    assert_eq!(scroll, (line * row as f32 - available / 2.).max(px(0.)));
}
