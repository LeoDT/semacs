//! Window furniture for the single M0 frame: mode line and echo area.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::echo::EchoArea;

pub fn mode_line(buffer: &Buffer, cx: &App) -> impl IntoElement {
    let (line, column) = buffer.line_column(cx);
    let dirty = if buffer.is_modified() { " *" } else { "" };
    let name = buffer.display_name();
    let format = buffer.format().as_str();

    h_flex()
        .id("mode-line")
        .h(px(24.0))
        .w_full()
        .px_3()
        .gap_3()
        .items_center()
        .bg(cx.theme().tab_bar)
        .text_color(cx.theme().muted_foreground)
        .text_sm()
        .child(format!("{name}{dirty}"))
        .child(format!("L{line}:{column}"))
        .child(format)
}

pub fn echo_area(echo: &EchoArea, cx: &App) -> impl IntoElement {
    v_flex()
        .id("echo-area")
        .h(px(24.0))
        .w_full()
        .px_3()
        .justify_center()
        .bg(cx.theme().background)
        .text_color(cx.theme().muted_foreground)
        .text_sm()
        .child(echo.message().to_owned())
}
