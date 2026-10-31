use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub(crate) fn init(cx: &mut App) {
    cx.set_prompt_builder(|_, message, detail, answers, handle, window, cx| {
        let prompt = cx.new(|cx| Confirmation {
            message: message.to_owned().into(),
            detail: detail.map(|detail| detail.to_owned().into()),
            answers: answers.to_vec(),
            selected: answers
                .iter()
                .position(PromptButton::is_cancel)
                .unwrap_or(0),
            focus: cx.focus_handle(),
        });
        handle.with_view(prompt, window, cx)
    });
}

struct Confirmation {
    message: SharedString,
    detail: Option<SharedString>,
    answers: Vec<PromptButton>,
    selected: usize,
    focus: FocusHandle,
}

impl Confirmation {
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let cancel = key.key == "escape"
            || (key.key == "g"
                && key.modifiers.control
                && !key.modifiers.alt
                && !key.modifiers.platform);
        if cancel {
            if let Some(index) = self.answers.iter().position(PromptButton::is_cancel) {
                cx.emit(PromptResponse(index));
            }
        } else if !key.modifiers.control && !key.modifiers.alt && !key.modifiers.platform {
            match key.key.as_str() {
                "tab" | "left" | "right" if !self.answers.is_empty() => {
                    let backwards = key.key == "left" || (key.key == "tab" && key.modifiers.shift);
                    let count = self.answers.len();
                    self.selected = (self.selected + if backwards { count - 1 } else { 1 }) % count;
                    cx.notify();
                }
                "enter" | "return" | "space" if !self.answers.is_empty() => {
                    cx.emit(PromptResponse(self.selected));
                }
                _ => {}
            }
        }
        cx.stop_propagation();
    }
}

impl EventEmitter<PromptResponse> for Confirmation {}

impl Focusable for Confirmation {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Confirmation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.answers.iter().enumerate().map(|(index, answer)| {
            Button::new(("confirmation-answer", index))
                .label(answer.label().clone())
                .when(index == self.selected, |button| button.primary())
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(PromptResponse(index))))
        });
        div()
            .id("confirmation")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.35))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(
                v_flex()
                    .w(px(440.))
                    .max_w(relative(0.9))
                    .p_4()
                    .gap_3()
                    .rounded_lg()
                    .bg(cx.theme().background)
                    .text_color(cx.theme().foreground)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(div().text_lg().child(self.message.clone()))
                    .children(
                        self.detail
                            .clone()
                            .map(|detail| div().text_sm().child(detail)),
                    )
                    .child(h_flex().justify_end().gap_2().children(buttons)),
            )
    }
}
