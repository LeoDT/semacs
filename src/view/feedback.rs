use super::*;

#[derive(Clone)]
pub(crate) enum Feedback {
    Message(SharedString),
    Hint(SharedString),
    Prefix(SharedString),
    Clear,
}

impl Feedback {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Message(text) | Self::Hint(text) | Self::Prefix(text) => Some(text),
            Self::Clear => None,
        }
    }
}

impl EventEmitter<Feedback> for BufferView {}

#[derive(PartialEq)]
struct AnnotationKey {
    layout: LayoutKey,
    bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    viewport_height: Pixels,
    message: SharedString,
    color: Hsla,
}

pub(super) struct AnnotationLayout {
    key: AnnotationKey,
    lines: Arc<[(Point<Pixels>, ShapedLine)]>,
}

impl BufferView {
    pub(crate) fn publish_feedback(&mut self, feedback: Feedback, cx: &mut Context<Self>) {
        self.message_task = None;
        *self.annotation_cache.borrow_mut() = None;
        self.message = feedback
            .text()
            .map(|text| SharedString::from(text.to_owned()));
        cx.emit(feedback);
        cx.notify();
    }

    pub(crate) fn clear_message(&mut self, cx: &mut Context<Self>) {
        self.publish_feedback(Feedback::Clear, cx);
    }

    pub(crate) fn set_hint(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.publish_feedback(Feedback::Hint(text.into().into()), cx);
        self.message_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            view.update(cx, |view, cx| view.clear_message(cx)).ok();
        }));
    }

    pub(crate) fn enable_message_annotation(&mut self) {
        self.annotate_message = true;
    }

    pub(super) fn annotation_lines(
        &self,
        window: &Window,
        cx: &App,
    ) -> Arc<[(Point<Pixels>, ShapedLine)]> {
        if !self.annotate_message {
            return Arc::from([]);
        }
        let Some(message) = self.message() else {
            return Arc::from([]);
        };
        let Some(layout) = self.viewport_layout() else {
            return Arc::from([]);
        };
        let end = ByteOffset(self.display_snapshot().text().len());
        let Some(row) = layout
            .rows
            .iter()
            .find(|row| row.source.start.0 + row.source.row.content_len == end.0)
        else {
            return Arc::from([]);
        };
        let Some(x) = row.x_for_offset(end) else {
            return Arc::from([]);
        };
        let key = AnnotationKey {
            layout: layout.key.clone(),
            bounds: layout.bounds,
            scroll: layout.scroll,
            viewport_height: window.viewport_size().height,
            message: self.message.clone().unwrap(),
            color: cx.theme().muted_foreground,
        };
        if let Some(cached) = self.annotation_cache.borrow().as_ref()
            && cached.key == key
        {
            return cached.lines.clone();
        }
        let origin = layout.origin(row);
        let mut position = point(origin.x + x, origin.y);
        let mut measure = FontMeasure::new(cx.text_system().clone(), self.settings.clone());
        let shape = |text: &str| {
            window.text_system().shape_line(
                text.to_owned().into(),
                self.settings.font_size,
                &[TextRun {
                    len: text.len(),
                    font: self.settings.font.clone(),
                    color: cx.theme().muted_foreground,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
        };
        let mut result = Vec::new();
        let mut line = String::new();
        let mut width = px(0.);
        let max_bottom = layout.bounds.top() + window.viewport_size().height * 0.25;
        let display = format!(" [{}]", message);
        for grapheme in display.graphemes(true) {
            let advance = measure.width(if grapheme == "\n" { " " } else { grapheme });
            if grapheme == "\n" || position.x + width + advance > layout.bounds.right() {
                if !line.is_empty() {
                    let shaped = shape(&line);
                    result.push((position, shaped));
                    line.clear();
                }
                position = point(layout.bounds.left(), position.y + self.settings.line_height);
                width = px(0.);
                if position.y >= max_bottom {
                    break;
                }
                if grapheme == "\n" {
                    continue;
                }
            }
            line.push_str(grapheme);
            width += advance;
        }
        if !line.is_empty() {
            result.push((position, shape(&line)));
        }
        let lines: Arc<[_]> = result.into();
        *self.annotation_cache.borrow_mut() = Some(AnnotationLayout {
            key,
            lines: lines.clone(),
        });
        lines
    }

    pub(crate) fn minibuffer_height(&self, window: &Window, cx: &App) -> Pixels {
        let height = self
            .annotation_lines(window, cx)
            .last()
            .and_then(|(position, _)| {
                self.viewport_layout()
                    .map(|layout| position.y - layout.bounds.top() + self.settings.line_height)
            })
            .unwrap_or(self.settings.line_height);
        (height + PADDING * 2.).min(
            (window.viewport_size().height * 0.25).max(self.settings.line_height + PADDING * 2.),
        )
    }
}
