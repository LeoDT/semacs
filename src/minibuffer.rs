use gpui_kit::*;

use crate::buffer::Buffer;
use crate::completion::CandidateList;
use crate::editing::{EditTarget, KillRing};
use crate::text::ByteOffset;
use crate::view::BufferView;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;

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
            .line_height(line_height);
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
                    .text_color(cx.theme().muted_foreground)
                    .child(count),
            );
            for index in list.visible_range(rows.min(8)) {
                let item = &list.items[index];
                let highlight = HighlightStyle {
                    color: Some(cx.theme().primary),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                };
                panel = panel.child(
                    div()
                        .h(line_height)
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .when(list.selected == Some(index), |row| {
                            row.bg(cx.theme().accent)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(
                                    StyledText::new(item.candidate.label.clone()).with_highlights(
                                        item.highlights
                                            .iter()
                                            .cloned()
                                            .map(|range| (range, highlight)),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .max_w(relative(0.5))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(cx.theme().muted_foreground)
                                .child(item.candidate.annotation.clone()),
                        ),
                );
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
                        |input| input.border_color(cx.theme().primary),
                    )
                    .child(self.editor.clone()),
            )
            .when(self.candidates.is_some(), |mini| mini.child(panel))
    }
}
