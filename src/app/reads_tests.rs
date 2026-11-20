use super::*;
use crate::buffer::Buffer;
use crate::git::Repository;
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
use std::path::PathBuf;

fn open(buffer: Buffer, cx: &mut TestAppContext) -> (VisualTestContext, Entity<AppShell>) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    draw(&mut visual);
    (visual, shell)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn candidate_height_only_grows_until_the_read_ends(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(Buffer::scratch(), cx);
    let large = size(px(400.), px(400.));
    visual.simulate_resize(large);
    let begin_read = |visual: &mut VisualTestContext| {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let origin = shell.active_view().clone();
                shell.begin_read(ReadKind::Command, &origin, "forward-word", window, cx);
            });
        });
        draw(visual);
    };
    let bounds = |visual: &mut VisualTestContext| visual.debug_bounds("minibuffer").unwrap();
    begin_read(&mut visual);
    let initial = bounds(&mut visual);
    assert!(visual.debug_bounds("candidate-row-0").is_some());
    assert!(visual.debug_bounds("candidate-row-1").is_none());

    visual.simulate_keystrokes("ctrl-a ctrl-k");
    draw(&mut visual);
    let expanded = bounds(&mut visual);
    assert!(expanded.top() < initial.top());
    assert!(visual.debug_bounds("candidate-row-7").is_some());
    assert!(visual.debug_bounds("candidate-row-8").is_none());

    visual.simulate_input("forward-word");
    draw(&mut visual);
    assert_eq!(bounds(&mut visual), expanded);
    assert!(visual.debug_bounds("candidate-row-1").is_none());
    visual.simulate_input("zzz");
    draw(&mut visual);
    assert_eq!(bounds(&mut visual), expanded);
    assert!(visual.debug_bounds("candidate-row-0").is_none());

    visual.simulate_resize(size(px(400.), px(140.)));
    draw(&mut visual);
    let resized = bounds(&mut visual);
    assert!(resized.size.height < expanded.size.height);
    assert_eq!(resized.bottom(), px(140.));
    visual.simulate_resize(large);
    draw(&mut visual);
    assert_eq!(bounds(&mut visual), expanded);

    visual.simulate_keystrokes("ctrl-g");
    draw(&mut visual);
    assert!(visual.debug_bounds("minibuffer").is_none());
    begin_read(&mut visual);
    assert_eq!(bounds(&mut visual), initial);
}

#[gpui::test]
fn file_reads_of_generated_buffers_start_at_the_repository_root(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join("semacs-read-repository-root");
    let repository = Repository {
        root: root.clone(),
        git_dir: root.join(".git"),
    };
    let (mut visual, shell) = open(Buffer::status(repository), cx);
    let view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).file_read_directory(&view, cx),
            PathBuf::from(&root)
        )
    });
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            files::directory_input(&root)
        )
    });
}

#[gpui::test]
fn save_of_an_unvisited_buffer_uses_the_buffer_name_in_the_default_directory(
    cx: &mut TestAppContext,
) {
    let directory =
        std::env::temp_dir().join(format!("semacs-unvisited-save-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let (mut visual, shell) = open(Buffer::named("draft"), cx);
    shell.update(&mut visual, |shell, _| {
        shell.startup_directory = directory.clone();
    });
    visual.simulate_input("dirty");
    visual.simulate_keystrokes("ctrl-x ctrl-s");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            files::directory_input(&directory)
        )
    });
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    let expected = directory.join("draft");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(expected.as_path())
        );
    });
    assert_eq!(std::fs::read(&expected).unwrap(), b"dirty");
    std::fs::remove_dir_all(&directory).unwrap();
}
