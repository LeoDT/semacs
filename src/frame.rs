//! Window furniture for the app shell: file metadata and echo area.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

pub fn mode_line(
    name: String,
    format: crate::files::FileFormat,
    modified: bool,
    position: (u32, usize),
    cx: &App,
) -> impl IntoElement {
    let format = format.as_str();
    let name = format!("{} {name}", if modified { "**" } else { "--" });
    let position = format!("L{} C{}", position.0, position.1);

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
        .child(position)
}

pub fn echo_area(message: &str, cx: &App) -> impl IntoElement {
    v_flex()
        .id("echo-area")
        .h(px(24.0))
        .w_full()
        .px_3()
        .justify_center()
        .bg(cx.theme().background)
        .text_color(cx.theme().muted_foreground)
        .text_sm()
        .child(message.to_owned())
}
