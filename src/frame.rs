//! Window furniture for the app shell: file metadata and echo area.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

pub fn mode_line(
    name: String,
    format: crate::files::FileFormat,
    modified: bool,
    position: (u32, usize),
    selected: bool,
    cx: &App,
) -> impl IntoElement + Styled {
    let format = format.as_str();
    let name = format!("{} {name}", if modified { "**" } else { "--" });
    let position = format!("L{} C{}", position.0, position.1);
    let (background, foreground) = if selected {
        (cx.theme().accent, cx.theme().accent_foreground)
    } else {
        (cx.theme().tab_bar, cx.theme().muted_foreground)
    };

    h_flex()
        .id("mode-line")
        .h(px(24.0))
        .w_full()
        .px_3()
        .gap_3()
        .items_center()
        .bg(background)
        .text_color(foreground)
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

#[cfg(test)]
mod tests {
    use super::mode_line;
    use crate::files::FileFormat;
    use gpui_kit as gpui;
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    use gpui_kit::{Styled, TestAppContext};

    #[gpui::test]
    fn mode_line_distinguishes_selection_in_light_and_dark_themes(cx: &mut TestAppContext) {
        cx.update(crate::app::init);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            cx.update(|cx| {
                Theme::change(mode, None, cx);
                let mut selected = mode_line(
                    "buffer".into(),
                    FileFormat::default(),
                    false,
                    (1, 0),
                    true,
                    cx,
                );
                let mut other = mode_line(
                    "buffer".into(),
                    FileFormat::default(),
                    false,
                    (1, 0),
                    false,
                    cx,
                );
                assert_eq!(selected.style().background, Some(cx.theme().accent.into()));
                assert_eq!(
                    selected.style().text.color,
                    Some(cx.theme().accent_foreground)
                );
                assert_eq!(other.style().background, Some(cx.theme().tab_bar.into()));
                assert_eq!(other.style().text.color, Some(cx.theme().muted_foreground));
                assert_ne!(selected.style().background, other.style().background);
                assert_ne!(selected.style().text.color, other.style().text.color);
            });
        }
    }
}
