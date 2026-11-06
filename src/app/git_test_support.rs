use super::AppShell;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::diff::tests::Repository as Fixture;
use crate::files::{DecodedFile, FileFormat};
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};

use std::time::{Duration, Instant};

pub(super) fn open(
    repo: &Fixture,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        Some(repo.0.join("a")),
        DecodedFile {
            text: "unsaved editor text\n".into(),
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

pub(super) fn command(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        })
    });
}

pub(super) fn until(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    predicate: impl Fn(&AppShell) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if !visual.windows().is_empty() {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        if visual.read(|cx| predicate(shell.read(cx))) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "status operation did not reach its barrier"
        );
        std::thread::yield_now();
    }
}
pub(super) fn wait(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    until(visual, shell, |shell| {
        shell.git_task.is_none() && shell.buffers.iter().all(|slot| !slot.save.is_saving())
    });
}
pub(super) fn gate(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
) -> futures_channel::oneshot::Sender<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.status_read_gate = Some(receive)));
    send
}
pub(super) fn repo() -> Fixture {
    let repo = Fixture::new();
    repo.write("a", "disk base\n");
    repo.commit();
    repo.write("a", "disk changed\n");
    repo
}
