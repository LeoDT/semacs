use std::time::Instant;

use gpui_kit::{Entity, VisualTestContext};
use semacs::view::BufferView;

pub fn draw(cx: &mut VisualTestContext, force: bool) {
    cx.update(|window, cx| {
        if force {
            window.refresh();
        }
        window.draw(cx).clear(cx);
    });
}

pub fn ready(cx: &mut VisualTestContext, editor: &Entity<BufferView>) {
    for _ in 0..8 {
        if cx.read(|cx| editor.read(cx).layout_ready()) {
            return;
        }
        cx.run_until_parked();
        draw(cx, false);
    }
    assert!(
        cx.read(|cx| editor.read(cx).layout_ready()),
        "layout did not settle"
    );
}

pub fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
