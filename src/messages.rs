use crate::buffer::Buffer;
use crate::text::{ByteOffset, Point};

pub(crate) const LOG_LINES: u32 = 1000;

pub(crate) struct Messages {
    pub buffer: Buffer,
    last: Option<(String, usize, usize)>,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            buffer: Buffer::messages(),
            last: None,
        }
    }
}

impl Messages {
    pub fn push(&mut self, message: &str) {
        if message.is_empty() {
            return;
        }
        let end = self.buffer.text().len();
        let (start, count) = match &self.last {
            Some((previous, start, count)) if previous == message && !message.contains('\n') => {
                (*start, count.saturating_add(1))
            }
            _ => (end, 1),
        };
        let mut entry = message.to_owned();
        if count > 1 {
            entry.push_str(&format!(" [{count} times]"));
        }
        if !entry.ends_with('\n') {
            entry.push('\n');
        }
        self.buffer
            .update_message_log(ByteOffset(start)..ByteOffset(end), &entry);
        let text = self.buffer.text();
        let excess = text.max_point().row.saturating_sub(LOG_LINES);
        let removed = text.point_to_offset(Point::new(excess, 0));
        if removed > 0 {
            self.buffer
                .update_message_log(ByteOffset(0)..ByteOffset(removed), "");
        }
        self.last = (removed <= start).then(|| (message.to_owned(), start - removed, count));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_keeps_multiline_messages_and_folds_only_adjacent_repeats() {
        let mut log = Messages::default();
        log.push("one\ntwo");
        log.push("one\ntwo");
        log.push("other");
        log.push("one\ntwo");
        assert_eq!(
            log.buffer.text().to_string(),
            "one\ntwo\none\ntwo\nother\none\ntwo\n"
        );
        assert!(log.buffer.is_read_only());
        assert!(!log.buffer.is_modified());
    }

    #[test]
    fn generated_updates_preserve_readonly_selection_and_do_not_accumulate_undo() {
        let mut log = Messages::default();
        log.push("first");
        log.buffer.set_mark(ByteOffset(1)).unwrap();
        log.push("second");
        assert!(log.buffer.mark_active());
        assert_eq!(log.buffer.mark(), Some(ByteOffset(1)));
        assert_eq!(
            log.buffer.edit(crate::buffer::Edit {
                snapshot: log.buffer.snapshot_id(),
                range: ByteOffset(0)..ByteOffset(0),
                text: "bad",
                source: crate::buffer::EditSource::Command,
            }),
            Err(crate::buffer::EditError::ReadOnly)
        );
        log.buffer.set_read_only(false);
        assert!(log.buffer.undo().unwrap().is_none());
        assert!(!log.buffer.is_modified());
    }

    #[test]
    fn pruning_is_by_lines_and_preserves_utf8_and_repeat_ranges() {
        let mut log = Messages::default();
        for i in 0..1005 {
            log.push(&format!("你{i}"));
        }
        assert!(log.buffer.text().to_string().starts_with("你5\n"));
        assert_eq!(log.buffer.text().max_point().row, LOG_LINES);
        log.push("你1004");
        assert!(
            log.buffer
                .text()
                .to_string()
                .ends_with("你1004 [2 times]\n")
        );
        log.push(&"长\n".repeat(1100));
        assert_eq!(log.buffer.text().max_point().row, LOG_LINES);
        log.push("after");
        assert!(log.buffer.text().to_string().ends_with("after\n"));
    }
}
