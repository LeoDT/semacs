use super::*;
use crate::buffer::{Edit, EditError, EditSource};
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};
use std::path::PathBuf;

fn repository() -> Repository {
    Repository {
        root: PathBuf::from("/test-process"),
        git_dir: PathBuf::from("/test-process/.git"),
    }
}

fn record(operation: u64, output: &[u8]) -> CommitRecord {
    CommitRecord {
        operation,
        repository: repository(),
        phase: CommitPhase::Running,
        output: output.to_vec(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn projection_preserves_raw_output_and_has_stable_section_identities() {
    let repo = repository();
    let first = record(41, "hello\r\n中文\t\u{1b}[31m".as_bytes());
    let mut second = record(42, b"");
    second.phase = CommitPhase::Failed {
        started: false,
        diagnostic: "no helper".into(),
    };
    let (text, sections, operations) = render(&repo, [&first, &second].into_iter());
    assert!(text.contains("hello\n中文\t\\u{1b}[31m"));
    assert!(text.contains("not started  git commit  [42]\nno helper"));
    assert_eq!(first.output, "hello\r\n中文\t\u{1b}[31m".as_bytes());
    assert_eq!(operations, [41, 42]);
    assert_eq!(sections.nodes()[1].key, "commit:41");
    assert_eq!(sections.nodes()[2].key, "commit:42");
    let mut buffer = Buffer::process(repo);
    buffer.install_process(&text, sections, operations);
    assert_eq!(buffer.process_at_point(), None);
    buffer
        .set_point(ByteOffset(text.find("hello").unwrap()))
        .unwrap();
    assert_eq!(buffer.process_at_point(), Some(41));
    assert_eq!(
        buffer.edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(0),
            text: "bad",
            source: EditSource::Command,
        }),
        Err(EditError::ReadOnly)
    );
    buffer.set_point(ByteOffset(text.len())).unwrap();
    assert_eq!(buffer.process_at_point(), Some(42));
    let saved = buffer.begin_save();
    let later = record(42, b"later output");
    let (text, sections, operations) = render(&repository(), [&first, &later].into_iter());
    buffer.install_process(&text, sections, operations);
    buffer.mark_saved_at(saved).unwrap();
    assert!(buffer.is_modified());
    let current = buffer.begin_save();
    buffer.mark_saved_at(current).unwrap();
    assert!(!buffer.is_modified());
    let mut shared = buffer.new_view();
    shared.detach_process();
    assert_eq!(buffer.mode(), ModeId::FUNDAMENTAL);
    assert!(!buffer.is_read_only());
    assert_eq!(buffer.repository(), None);
    assert_eq!(buffer.process_at_point(), None);
    assert!(buffer.sections().is_none());
}

#[gpui::test]
fn process_display_reuses_and_quits_locally_without_giving_plain_text_bindings(
    cx: &mut TestAppContext,
) {
    use crate::app::git_test_support::command;
    cx.update(crate::app::init);
    let repo = repository();
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::status(repo.clone()), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    command(&mut visual, &shell, Command::MagitProcessBuffer);
    let process = visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_PROCESS
        );
        assert!(
            shell
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("Git processes:")
        );
        shell.active_buffer_id()
    });
    command(&mut visual, &shell, Command::MagitProcessBuffer);
    visual.read(|cx| assert_eq!(shell.read(cx).active_buffer_id(), process));
    command(&mut visual, &shell, Command::MagitProcessKill);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("No process at point")
        )
    });
    visual.simulate_keystrokes("q");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_STATUS
        );
        assert_eq!(shell.read(cx).panes.leaves().len(), 1);
    });
    visual.simulate_keystrokes("$");
    visual.read(|cx| assert_eq!(shell.read(cx).active_buffer_id(), process));
    visual.simulate_keystrokes("q");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("plain"), window, cx);
            shell.activate(index, window, cx);
        });
    });
    visual.simulate_input("$kq");
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "$kq"
        );
    });
}
