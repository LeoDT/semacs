use super::*;

pub(crate) type SectionScrollAnchor = (crate::section::SectionAnchor, Pixels);

impl BufferView {
    pub(crate) fn show_location(
        &mut self,
        point: ByteOffset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.target.show_point(point);
        self.target.clear_pending();
        self.scroll_anchor = None;
        self.pending_scroll = None;
        self.reveal_caret = true;
        self.invalidate_input_registration();
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    /// Moves to a section heading. When the section is a hunk that does not fit
    /// in the window, its heading is pinned to the top
    /// (`magit-hunk-set-window-start`); the caret reveal still runs, so a
    /// horizontally scrolled heading is also brought into view.
    pub(crate) fn show_hunk(
        &mut self,
        range: Range<ByteOffset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_location(range.start, window, cx);
        self.pending_scroll = Some(PendingScroll::Hunk(range));
    }

    pub(crate) fn show_line_column(
        &mut self,
        line: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.buffer().text();
        let row = line.saturating_sub(1).min(text.max_point().row as usize) as u32;
        let start = text.point_to_offset(crate::text::Point::new(row, 0));
        let mut offset = start + column.min(text.line_len(row) as usize);
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        self.show_location(ByteOffset(offset), window, cx);
    }

    pub(crate) fn scroll_horizontal(&mut self, left: bool, cx: &mut Context<Self>) {
        let width = self
            .content_bounds
            .map_or(px(160.), |bounds| bounds.size.width * 0.5);
        self.scroll_by(point(if left { width } else { -width }, px(0.)), cx);
    }

    pub(crate) fn section_scroll_anchor(&self) -> Option<SectionScrollAnchor> {
        let index = self.index.as_ref()?;
        let first =
            (f32::from(self.scroll.y) / f32::from(self.settings.line_height)).floor() as usize;
        let offset = index.row(first)?.start;
        let sections = self.buffer().sections()?;
        Some((
            sections.anchor(&self.buffer().text(), offset),
            self.scroll.y - self.settings.line_height * first as f32,
        ))
    }

    pub(crate) fn restore_section_scroll(
        &mut self,
        anchor: Option<SectionScrollAnchor>,
        cx: &mut Context<Self>,
    ) {
        self.pending_scroll = None;
        if let (Some((anchor, remainder)), Some(sections)) = (anchor, self.buffer().sections()) {
            self.scroll_anchor =
                Some((sections.resolve(&self.buffer().text(), &anchor), remainder));
            self.reveal_caret = false;
            cx.notify();
        }
    }
}
