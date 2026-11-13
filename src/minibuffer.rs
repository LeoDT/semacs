use gpui_kit::*;

use crate::buffer::Buffer;
use crate::completion::{AnnotationPlacement, CandidateList};
use crate::editing::{EditTarget, KillRing};
use crate::text::ByteOffset;
use crate::theme::{StyleRole, ThemeSettings};
use crate::view::BufferView;
use gpui_kit::prelude::FluentBuilder;
use unicode_width::UnicodeWidthStr;

pub struct Minibuffer {
    editor: Entity<BufferView>,
    candidates: Option<CandidateList>,
    _editor_subscription: Subscription,
}

impl Minibuffer {
    pub fn new(prompt: &str, initial: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let target = EditTarget::with_kill_ring(
            Buffer::minibuffer(prompt, initial),
            cx.default_global::<KillRing>().clone(),
        )
        .with_field_boundary(ByteOffset(prompt.len()))
        .expect("valid prompt field boundary");
        let editor = cx.new(|cx| BufferView::with_target(target, cx));
        editor.update(cx, |view, cx| {
            view.enable_message_annotation();
            view.install_input_lifecycle(window, cx);
        });
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            candidates: None,
            _editor_subscription: subscription,
        }
    }

    pub(crate) fn set_candidates(&mut self, candidates: CandidateList, cx: &mut Context<Self>) {
        self.candidates = Some(candidates);
        cx.notify();
    }

    pub fn editor(&self) -> &Entity<BufferView> {
        &self.editor
    }

    pub fn prompt(&self, cx: &App) -> String {
        let buffer = self.editor.read(cx).buffer();
        buffer
            .text()
            .chunks_in_range(0..buffer.read_only_prefix().0)
            .collect()
    }

    pub fn contents(&self, cx: &App) -> String {
        let buffer = self.editor.read(cx).buffer();
        buffer
            .text()
            .chunks_in_range(buffer.read_only_prefix().0..buffer.text().len())
            .collect()
    }
}

impl Focusable for Minibuffer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for Minibuffer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ThemeSettings::global(cx);
        let completion = theme.style(StyleRole::Completion);
        let selected = theme.style(StyleRole::CompletionSelected);
        let annotation = theme.style(StyleRole::CompletionAnnotation);
        let matched = theme.style(StyleRole::CompletionMatch);
        let line_height = self.editor.read(cx).line_height();
        let input_height = self.editor.read(cx).minibuffer_height(window, cx);
        let fonts = cx.global::<crate::appearance::FontSettings>();
        let available = window.viewport_size().height - input_height - px(72.) - line_height;
        let rows = (f32::from(available.max(px(0.))) / f32::from(line_height)).floor() as usize;
        let mut panel = div()
            .w_full()
            .overflow_hidden()
            .font_family(fonts.family().to_owned())
            .text_size(fonts.size())
            .line_height(line_height)
            .bg(completion.background)
            .text_color(completion.foreground);
        if let Some(list) = &self.candidates {
            let count = if list.loading && list.items.is_empty() {
                "Updating candidates…".to_owned()
            } else if list.items.is_empty() {
                "No match".to_owned()
            } else {
                format!(
                    "{} / {}",
                    list.selected
                        .map_or("Input".to_owned(), |index| (index + 1).to_string()),
                    list.items.len()
                )
            };
            panel = panel.child(
                div()
                    .h(px(24.))
                    .px_3()
                    .text_sm()
                    .text_color(annotation.foreground)
                    .child(count),
            );
            let visible = list.visible_range(rows.min(8));
            let column = match list.annotation {
                AnnotationPlacement::MarginaliaLeft => {
                    let font_id = window.text_system().resolve_font(&fonts.font());
                    let advance = window
                        .text_system()
                        .ch_advance(font_id, fonts.size())
                        .unwrap_or(px(0.));
                    let widest = list.items[visible.clone()]
                        .iter()
                        .map(|item| UnicodeWidthStr::width(item.candidate.label.as_str()))
                        .max()
                        .unwrap_or(0);
                    Some((advance * annotation_column(widest) as f32, advance * 2.))
                }
                AnnotationPlacement::Right => None,
            };
            for index in visible {
                let item = &list.items[index];
                let highlight = HighlightStyle {
                    color: Some(matched.foreground),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                };
                let label = StyledText::new(item.candidate.label.clone()).with_highlights(
                    item.highlights
                        .iter()
                        .cloned()
                        .map(|range| (range, highlight)),
                );
                let annotation_text = item.candidate.annotation.clone();
                let mut row = div()
                    .h(line_height)
                    .w_full()
                    .flex()
                    .items_center()
                    .px_3()
                    .debug_selector(move || format!("candidate-row-{index}"))
                    .when(list.selected == Some(index), |row| {
                        row.bg(selected.background).text_color(selected.foreground)
                    });
                row = match column {
                    Some((start, separator)) => row
                        .child(
                            div()
                                .flex_none()
                                .min_w(start)
                                .whitespace_nowrap()
                                .debug_selector(move || format!("candidate-label-{index}"))
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .pl(separator)
                                .debug_selector(move || format!("candidate-annotation-{index}"))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(annotation.foreground)
                                .child(annotation_text),
                        ),
                    None => row
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .debug_selector(move || format!("candidate-label-{index}"))
                                .child(label),
                        )
                        .child(
                            div()
                                .max_w(relative(0.5))
                                .debug_selector(move || format!("candidate-annotation-{index}"))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(annotation.foreground)
                                .child(annotation_text),
                        ),
                };
                panel = panel.child(row);
            }
        }
        div()
            .id("minibuffer")
            .debug_selector(|| "minibuffer".into())
            .w_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .h(input_height)
                    .w_full()
                    .when(self.candidates.is_some(), |input| {
                        input.border_l_2().border_color(transparent_black())
                    })
                    .when(
                        self.candidates
                            .as_ref()
                            .is_some_and(|list| list.selected.is_none()),
                        |input| input.border_color(matched.foreground),
                    )
                    .child(self.editor.clone()),
            )
            .when(self.candidates.is_some(), |mini| mini.child(panel))
    }
}

/// Marginalia's left annotation column for the displayed candidates: at least 20
/// cells, rounded up to the next multiple of ten.
fn annotation_column(widest: usize) -> usize {
    widest.div_ceil(10).max(2) * 10
}

#[cfg(test)]
mod tests {
    use super::annotation_column;

    #[test]
    fn annotation_column_rounds_up_to_ten_cells_with_a_twenty_cell_minimum() {
        for (widest, expected) in [
            (0, 20),
            (5, 20),
            (19, 20),
            (20, 20),
            (21, 30),
            (29, 30),
            (30, 30),
            (31, 40),
        ] {
            assert_eq!(annotation_column(widest), expected, "{widest}");
        }
    }
}
