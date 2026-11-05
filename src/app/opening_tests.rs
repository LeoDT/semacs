use super::*;
use crate::files::{DecodedFile, FileFormat};
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, WindowHandle, px, size};

fn setup(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<AppShell>,
    VisualTestContext,
    Entity<BufferView>,
    Entity<BufferView>,
) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let origin = window
        .read_with(cx, |shell, _| shell.active_view().clone())
        .unwrap();
    let target = cx.update(|cx| {
        cx.new(|cx| {
            BufferView::new(
                Buffer::new(
                    Some(PathBuf::from("target.txt")),
                    DecodedFile {
                        text: "中🙂abcdefghijk\tlast\n".repeat(2000),
                        format: FileFormat::default(),
                    },
                ),
                cx,
            )
        })
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    cx.update_window(window.into(), |shell, _, cx| {
        shell
            .downcast::<AppShell>()
            .unwrap()
            .update(cx, |shell, cx| {
                shell.opening = Some(Opening {
                    origin: origin.clone(),
                    view: target.clone(),
                    viewport: None,
                    location: None,
                });
                cx.notify();
            });
    })
    .unwrap();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        window
            .read_with(cx, |shell, _| shell.active_view().clone())
            .unwrap(),
        origin
    );
    assert_eq!(
        window
            .read_with(cx, |shell, _| shell.buffer_count())
            .unwrap(),
        1
    );
    assert!(origin.read_with(cx, |view, _| view.paint_layout().is_some()));
    (window, visual, origin, target)
}

#[gpui::test]
fn pending_open_keeps_the_origin_editable_until_a_prepared_target_can_be_activated(
    cx: &mut TestAppContext,
) {
    let (window, mut visual, origin, target) = setup(cx);
    visual.simulate_input("X");
    visual.run_until_parked();
    assert_eq!(
        window
            .read_with(cx, |shell, _| shell.active_view().clone())
            .unwrap(),
        target
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    target.read_with(cx, |view, _| {
        let layout = view.viewport_layout().unwrap();
        assert!(layout.visible_row_count() > 0);
        assert_eq!(layout.key().display, view.display_snapshot().id());
    });
    assert_eq!(
        origin.read_with(cx, |view, _| view.buffer().text().to_string()),
        "X"
    );
}

#[gpui::test]
fn a_new_read_session_cancels_layout_preparation_without_creating_a_ghost_buffer(
    cx: &mut TestAppContext,
) {
    let (window, visual, origin, target) = setup(cx);
    let weak = target.downgrade();
    drop(target);
    cx.update_window(window.into(), |shell, window, cx| {
        shell
            .downcast::<AppShell>()
            .unwrap()
            .update(cx, |shell, cx| {
                shell.begin_read(ReadKind::Command, &origin, "", window, cx);
            });
    })
    .unwrap();
    visual.run_until_parked();
    window
        .read_with(cx, |shell, _| {
            assert_eq!(shell.active_view(), &origin);
            assert_eq!(shell.buffer_count(), 1);
            assert!(shell.opening.is_none());
            assert!(shell.session.is_some());
        })
        .unwrap();
    assert!(weak.upgrade().is_none());
}

#[gpui::test]
fn resizing_before_publication_restarts_preparation_at_actual_body_bounds(cx: &mut TestAppContext) {
    let (window, mut visual, _, target) = setup(cx);
    visual.simulate_resize(size(px(580.), px(320.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        window
            .read_with(cx, |shell, _| shell.active_view().clone())
            .unwrap(),
        target
    );
    target.read_with(cx, |view, _| {
        let layout = view.viewport_layout().unwrap();
        assert_eq!(layout.key().settings.wrap_width, layout.bounds().size.width);
        assert!(layout.bounds().size.width > px(400.));
        assert!(layout.visible_row_count() > 0);
    });
}

#[gpui::test]
fn a_buffer_added_during_preparation_is_reused_instead_of_duplicated(cx: &mut TestAppContext) {
    let (window, visual, _, _) = setup(cx);
    let existing = cx
        .update_window(window.into(), |shell, window, cx| {
            shell
                .downcast::<AppShell>()
                .unwrap()
                .update(cx, |shell, cx| {
                    let index = shell.create_buffer(
                        Buffer::new_file(PathBuf::from("target.txt")),
                        window,
                        cx,
                    );
                    shell.buffer_view(index).unwrap().clone()
                })
        })
        .unwrap();
    visual.run_until_parked();
    window
        .read_with(cx, |shell, _| {
            assert_eq!(shell.active_view(), &existing);
            assert_eq!(shell.buffer_count(), 2);
            assert!(shell.opening.is_none());
        })
        .unwrap();
}
