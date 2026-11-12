use super::*;
use crate::app::git_test_support::*;
use crate::command::Command;
use crate::diff::tests::Repository as Fixture;
use crate::status::{Group, index::Outcome};
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};

fn base() -> String {
    (1..=60).map(|n| format!("line {n}\n")).collect()
}

fn opened(cx: &mut TestAppContext) -> (Fixture, VisualTestContext, Entity<AppShell>) {
    let repo = Fixture::new();
    repo.write("a", base());
    repo.commit();
    repo.write(
        "a",
        base()
            .replace("line 2\n", "first\n")
            .replace("line 50\n", "last\n"),
    );
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    (repo, visual, shell)
}

fn select_hunk(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    group: Group,
    nth: usize,
    body: bool,
) {
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        let buffer = view.read(cx).buffer();
        let sections = buffer.sections().unwrap();
        let node = sections
            .nodes()
            .iter()
            .filter(|node| node.key.starts_with(group.key()) && node.key.contains("/@@ "))
            .nth(nth)
            .unwrap();
        let point = if body { node.body } else { node.range.start };
        buffer.new_view().reveal_section(point);
        view.update(cx, |view, cx| view.show_location(point, window, cx));
    });
}

fn settled(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    until(visual, shell, |shell| {
        shell.script_task.is_none() && shell.git_task.is_none()
    });
}

#[gpui::test]
fn status_hunk_keys_stage_only_that_hunk_and_restore_both_views_without_stealing_focus(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    let source = visual.read(|cx| shell.read(cx).buffers[0].buffer.text().to_string());
    select_hunk(&mut visual, &shell, Group::Unstaged, 0, false);
    let first = visual.read(|cx| shell.read(cx).active_view().clone());
    let first_point = visual.read(|cx| first.read(cx).buffer().point());
    command(&mut visual, &shell, Command::SplitWindowRight);
    command(&mut visual, &shell, Command::OtherWindow);
    let second = visual.read(|cx| shell.read(cx).active_view().clone());
    select_hunk(&mut visual, &shell, Group::Unstaged, 1, true);
    visual.simulate_keystrokes("s");
    settled(&mut visual, &shell);
    assert_eq!(
        repo.git(&["show", ":a"]),
        base().replace("line 50\n", "last\n").as_bytes()
    );
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &second);
        assert_eq!(first.read(cx).buffer().point(), first_point);
        let buffer = second.read(cx).buffer();
        let sections = buffer.sections().unwrap();
        let node = &sections.nodes()[sections.current(buffer.point())];
        assert!(node.key.starts_with("unstaged/file/") && !node.key.contains("/@@ "));
        assert!(!node.collapsed);
        assert_eq!(
            shell.git_write_records().last().unwrap().outcome,
            Some(Outcome::Succeeded)
        );
        assert_eq!(shell.buffers[0].buffer.text().to_string(), source);
    });
    select_hunk(&mut visual, &shell, Group::Staged, 0, false);
    visual.simulate_keystrokes("u");
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), base().as_bytes());
    assert_eq!(
        std::fs::read(repo.0.join("a")).unwrap(),
        base()
            .replace("line 2\n", "first\n")
            .replace("line 50\n", "last\n")
            .as_bytes()
    );
}

#[gpui::test]
fn hunk_folds_and_each_view_point_and_scroll_survive_unchanged_refresh(cx: &mut TestAppContext) {
    let (_repo, mut visual, shell) = opened(cx);
    select_hunk(&mut visual, &shell, Group::Unstaged, 0, false);
    visual.simulate_keystrokes("tab");
    let first = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::SplitWindowRight);
    command(&mut visual, &shell, Command::OtherWindow);
    let second = visual.read(|cx| shell.read(cx).active_view().clone());
    select_hunk(&mut visual, &shell, Group::Unstaged, 1, true);
    visual.run_until_parked();
    let before = visual.read(|cx| {
        (
            first.read(cx).buffer().point(),
            second.read(cx).buffer().point(),
            first.read(cx).paint_layout().unwrap().scroll(),
            second.read(cx).paint_layout().unwrap().scroll(),
        )
    });
    visual.simulate_keystrokes("g");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &second);
        assert_eq!(
            (
                first.read(cx).buffer().point(),
                second.read(cx).buffer().point(),
                first.read(cx).paint_layout().unwrap().scroll(),
                second.read(cx).paint_layout().unwrap().scroll()
            ),
            before
        );
        let sections = second.read(cx).buffer().sections().unwrap();
        let hunks: Vec<_> = sections
            .nodes()
            .iter()
            .filter(|node| node.key.contains("/@@ "))
            .collect();
        assert!(hunks[0].collapsed);
        assert!(!hunks[1].collapsed);
        assert_eq!(hunks.len(), 2);
        assert!(hunks.iter().all(|node| node.hunk));
        for node in sections.nodes() {
            assert_eq!(node.hunk, node.key.contains("/@@ "), "{}", node.key);
        }
        assert_eq!(
            sections.visible_lines(),
            first.read(cx).buffer().sections().unwrap().visible_lines()
        );
    });
}

#[gpui::test]
fn delayed_js_hunk_return_cannot_retarget_another_hunk_or_apply_an_active_region(
    cx: &mut TestAppContext,
) {
    for region in [false, true] {
        let (repo, mut visual, shell) = opened(cx);
        select_hunk(&mut visual, &shell, Group::Unstaged, 0, true);
        let (send, receive) = futures_channel::oneshot::channel();
        visual.update(|_, cx| {
            shell.update(cx, |shell, _| shell.workflow_return_gate = Some(receive))
        });
        visual.simulate_keystrokes("s");
        select_hunk(&mut visual, &shell, Group::Unstaged, 1, true);
        if region {
            visual.update(|_, cx| {
                let view = shell.read(cx).active_view().read(cx);
                view.buffer()
                    .new_view()
                    .set_mark(crate::text::ByteOffset(0))
                    .unwrap();
            });
        }
        send.send(()).unwrap();
        settled(&mut visual, &shell);
        assert_eq!(repo.git(&["show", ":a"]), base().as_bytes());
        visual.read(|cx| assert_eq!(shell.read(cx).git_write_records().count(), 0));
    }
}

#[gpui::test]
fn accepted_hunk_patch_outlives_its_status_buffer_and_cannot_install_into_a_replacement(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    select_hunk(&mut visual, &shell, Group::Unstaged, 1, true);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.index_write_gate = Some(receive)));
    visual.simulate_keystrokes("s");
    until(&mut visual, &shell, |shell| shell.git_write_active());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let view = shell.active_view().clone();
            let index = shell.buffer_index(&view, cx).unwrap();
            let repo = shell.buffers[index].buffer.status_repository().unwrap();
            shell.remove_buffer(index, window, cx);
            shell.create_buffer(crate::buffer::Buffer::status(repo), window, cx);
        })
    });
    send.send(()).unwrap();
    settled(&mut visual, &shell);
    assert_eq!(
        repo.git(&["show", ":a"]),
        base().replace("line 50\n", "last\n").as_bytes()
    );
    visual.read(|cx| {
        let shell = shell.read(cx);
        let record = shell.git_write_records().last().unwrap();
        assert_eq!(record.outcome, Some(Outcome::Succeeded));
        assert!(!record.installed);
        assert!(
            shell
                .buffers
                .iter()
                .find(|slot| slot.buffer.status_repository().is_some())
                .unwrap()
                .buffer
                .text()
                .is_empty()
        );
    });
}
