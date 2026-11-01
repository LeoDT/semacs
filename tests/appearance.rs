mod support;

use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px};

use semacs::app::AppShell;
use semacs::appearance::FontSettings;
use semacs::buffer::Buffer;
use semacs::session::ReadKind;
use semacs::view::BufferView;
use support::*;

fn command(visual: &mut VisualTestContext, name: &str) {
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input(name);
    visual.simulate_keystrokes("enter");
    redraw(visual);
}

fn assert_font(visual: &VisualTestContext, view: &Entity<BufferView>, family: &str, size: f32) {
    visual.read(|cx| {
        let view = view.read(cx);
        let settings = &view.wrap_index().unwrap().key().settings;
        assert_eq!(settings.font.family.as_ref(), family);
        assert_eq!(settings.font_size, px(size));
        assert_eq!(settings.line_height, px(size * 1.5));
        assert!(view.layout_ready());
    });
}

fn minibuffer_editor(visual: &VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
    visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone()
    })
}

#[gpui::test]
fn font_commands_update_existing_hidden_and_new_views_and_minibuffers(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open(Buffer::new(None, decoded("first")), cx);
    let other = create(&mut visual, &shell, "second", None);
    activate(&mut visual, &shell, other);
    let second = active_view(&mut visual, &shell);
    command(&mut visual, "increase-font-size");
    assert_font(&visual, &second, "monospace", 17.);
    activate(&mut visual, &shell, 0);
    assert_font(&visual, &first, "monospace", 17.);

    command(&mut visual, "set-font-family");
    assert_eq!(prompt(&mut visual, &shell), "Font family: ");
    visual.simulate_input("serif");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_font(&visual, &first, "serif", 17.);
    activate(&mut visual, &shell, other);
    assert_font(&visual, &second, "serif", 17.);

    let third = create(&mut visual, &shell, "third", None);
    activate(&mut visual, &shell, third);
    let third_view = active_view(&mut visual, &shell);
    assert_font(&visual, &third_view, "serif", 17.);
    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    let editor = minibuffer_editor(&visual, &shell);
    assert_font(&visual, &editor, "serif", 17.);
    visual.simulate_keystrokes("ctrl-g");
    command(&mut visual, "reset-font-size");
    assert_font(&visual, &third_view, "serif", 16.);
    command(&mut visual, "decrease-font-size");
    assert_font(&visual, &third_view, "serif", 15.);
}

#[gpui::test]
fn font_family_cancel_and_invalid_input_preserve_the_active_configuration(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open(Buffer::scratch(), cx);
    command(&mut visual, "set-font-family");
    visual.simulate_input("serif");
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    assert_focused(&mut visual, &body);
    assert_font(&visual, &body, "monospace", 16.);

    command(&mut visual, "set-font-family");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().unwrap().kind(),
            &ReadKind::FontFamily
        );
        assert_eq!(cx.global::<FontSettings>().family(), "monospace");
        assert!(
            minibuffer_editor(&visual, &shell)
                .read(cx)
                .message()
                .unwrap()
                .contains("nonempty")
        );
    });
    visual.simulate_input("sans-serif");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_font(&visual, &body, "sans-serif", 16.);
    assert_focused(&mut visual, &body);
}

#[gpui::test]
fn resizing_preserves_document_state_and_history_and_reveals_point(cx: &mut TestAppContext) {
    let (mut visual, _, body) = open(Buffer::new(None, decoded(&"line 中🙂\n".repeat(1000))), cx);
    visual.simulate_keystrokes("alt->");
    visual.simulate_input("edited");
    visual.simulate_keystrokes("ctrl-space ctrl-b");
    redraw(&mut visual);
    let before = visual.read(|cx| {
        let buffer = body.read(cx).buffer();
        (
            buffer.snapshot_id(),
            buffer.point(),
            buffer.mark(),
            buffer.is_modified(),
        )
    });
    command(&mut visual, "increase-font-size");
    assert_font(&visual, &body, "monospace", 17.);
    visual.read(|cx| {
        let view = body.read(cx);
        let buffer = view.buffer();
        assert_eq!(
            (
                buffer.snapshot_id(),
                buffer.point(),
                buffer.mark(),
                buffer.is_modified()
            ),
            before
        );
        let layout = view.viewport_layout().unwrap();
        let caret = layout
            .bounds_for_range(buffer.point()..buffer.point(), view.wrap_index().unwrap())
            .unwrap();
        assert!(caret.top() >= layout.bounds().top());
        assert!(caret.bottom() <= layout.bounds().bottom());
    });
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            body.read(cx).buffer().text().to_string(),
            "line 中🙂\n".repeat(1000)
        )
    });
}

#[gpui::test]
fn live_settings_resize_an_existing_minibuffer_and_reject_out_of_range_commands(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, body) = open(Buffer::scratch(), cx);
    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    let editor = minibuffer_editor(&visual, &shell);
    let before = visual.read(|cx| editor.read(cx).viewport_layout().unwrap().bounds());
    visual.update(|_, cx| {
        let mut fonts = cx.global::<FontSettings>().clone();
        fonts.set_size(px(72.)).unwrap();
        cx.set_global(fonts);
    });
    redraw(&mut visual);
    assert_font(&visual, &editor, "monospace", 72.);
    visual.read(|cx| {
        let view = editor.read(cx);
        let bounds = view.viewport_layout().unwrap().bounds();
        assert_eq!(bounds.size.height, px(108.));
        assert!(bounds.size.height > before.size.height);
    });
    visual.simulate_input("increase-font-size");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_font(&visual, &body, "monospace", 72.);
    visual.read(|cx| {
        assert!(
            body.read(cx)
                .message()
                .unwrap()
                .contains("between 8 and 72")
        )
    });
    visual.update(|_, cx| {
        let mut fonts = cx.global::<FontSettings>().clone();
        fonts.set_size(px(8.)).unwrap();
        cx.set_global(fonts);
    });
    command(&mut visual, "decrease-font-size");
    assert_font(&visual, &body, "monospace", 8.);
    command(&mut visual, "reset-font-size");
    assert_font(&visual, &body, "monospace", 16.);
}
