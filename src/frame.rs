//! Window furniture for the app shell: file metadata and echo area.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

use crate::echo::EchoArea;

pub fn mode_line(name: String, format: crate::files::FileFormat, cx: &App) -> impl IntoElement {
    let format = format.as_str();

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
        .child(name)
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
