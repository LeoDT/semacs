//! Window furniture for the app shell: file metadata and echo area.

use crate::theme::{StyleRole, ThemeSettings};
use gpui_kit::component::{h_flex, v_flex};
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
    let colors = ThemeSettings::global(cx).style(if selected {
        StyleRole::ModeLine
    } else {
        StyleRole::ModeLineInactive
    });

    h_flex()
        .id("mode-line")
        .h(px(24.0))
        .flex_shrink_0()
        .overflow_hidden()
        .w_full()
        .px_3()
        .gap_3()
        .items_center()
        .bg(colors.background)
        .text_color(colors.foreground)
        .text_sm()
        .child(name)
        .child(format)
        .child(position)
}

pub fn echo_area(message: &str, max_height: Pixels, cx: &App) -> impl IntoElement {
    v_flex()
        .id("echo-area")
        .debug_selector(|| "echo-area".into())
        .min_h(px(24.0))
        .max_h(max_height.max(px(24.)))
        .flex_shrink_0()
        .overflow_hidden()
        .w_full()
        .px_3()
        .justify_start()
        .bg(ThemeSettings::global(cx)
            .style(StyleRole::EchoArea)
            .background)
        .text_color(
            ThemeSettings::global(cx)
                .style(StyleRole::EchoArea)
                .foreground,
        )
        .text_sm()
        .child(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::mode_line;
    use crate::files::FileFormat;
    use crate::theme::{StyleRole, ThemeSettings};
    use gpui_kit as gpui;
    use gpui_kit::{Styled, TestAppContext};

    #[gpui::test]
    fn mode_line_distinguishes_selection_in_light_and_dark_themes(cx: &mut TestAppContext) {
        cx.update(crate::app::init);
        for name in ["light", "dark"] {
            cx.update(|cx| {
                let mut settings = ThemeSettings::default();
                settings.select(name).unwrap();
                settings.install(cx);
                let selected_colors = ThemeSettings::global(cx).style(StyleRole::ModeLine);
                let other_colors = ThemeSettings::global(cx).style(StyleRole::ModeLineInactive);
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
                assert_eq!(
                    selected.style().background,
                    Some(selected_colors.background.into())
                );
                assert_eq!(
                    selected.style().text.color,
                    Some(selected_colors.foreground)
                );
                assert_eq!(
                    other.style().background,
                    Some(other_colors.background.into())
                );
                assert_eq!(other.style().text.color, Some(other_colors.foreground));
                assert_ne!(selected.style().background, other.style().background);
                assert_ne!(selected.style().text.color, other.style().text.color);
            });
        }
    }
}
