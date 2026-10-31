//! Emacs-compatible mark (marker) state for one editing target.
//!
//! The mark is not a plain offset: text changes move it. Rules verified against
//! GNU Emacs 31.1 (spikes/reference/RESULTS.md):
//!
//! * insertion before the mark shifts it, insertion at the mark leaves it
//!   before the inserted text (insertion type nil), insertion after it is inert;
//! * deleting a range that covers the mark collapses it to the deletion start;
//! * undo restores the old offset only when the mark is still at that start
//!   (Emacs records the collapse per change, not per undo group).
//!
//! Active state is separate: commands decide when the region is deactivated.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

#[derive(Default, Debug)]
pub struct MarkState {
    pos: Option<usize>,
    active: bool,
}

impl MarkState {
    pub fn pos(&self) -> Option<usize> {
        self.pos
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn set(&mut self, pos: usize) {
        self.pos = Some(pos);
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Apply the marker rule for replacing `range` with `new_len` bytes.
    pub fn after_replace(&mut self, range: Range<usize>, new_len: usize) {
        let Some(mark) = self.pos else {
            return;
        };
        let (start, end) = (range.start, range.end);
        let updated = if mark <= start {
            mark
        } else if mark >= end {
            mark - (end - start) + new_len
        } else {
            start
        };
        self.pos = Some(updated);
    }

    /// Offset to remember when deleting `range` swallows the mark. Replaying
    /// the deletion backwards restores it only if the mark is still at `start`.
    pub fn collapse_offset(&self, range: Range<usize>) -> Option<usize> {
        let mark = self.pos?;
        let (start, end) = (range.start, range.end);
        if start < end && start < mark && mark <= end {
            Some(mark - start)
        } else {
            None
        }
    }

    /// Undo of a deletion: reinserted text should bring a collapsed mark back.
    pub fn restore_collapsed(&mut self, start: usize, offset: usize) {
        if self.pos == Some(start) {
            self.pos = Some(start + offset);
        }
    }
}

pub type SharedMark = Rc<RefCell<MarkState>>;

pub fn new_mark() -> SharedMark {
    Rc::new(RefCell::new(MarkState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(pos: usize) -> MarkState {
        let mut mark = MarkState::default();
        mark.set(pos);
        mark
    }

    #[test]
    fn insert_before_at_and_after_mark() {
        let mut mark = marked(3);
        mark.after_replace(1..1, 1);
        assert_eq!(mark.pos(), Some(4));
        mark.after_replace(4..4, 1);
        assert_eq!(mark.pos(), Some(4));
        mark.after_replace(6..6, 2);
        assert_eq!(mark.pos(), Some(4));
    }

    #[test]
    fn deletion_collapses_to_start() {
        for mark_pos in [3, 4, 5] {
            let mut mark = marked(mark_pos);
            mark.after_replace(3..5, 0);
            assert_eq!(mark.pos(), Some(3), "mark {mark_pos} in 3..5");
        }
    }

    #[test]
    fn replacement_uses_new_length() {
        let mut mark = marked(6);
        mark.after_replace(2..4, 3);
        assert_eq!(mark.pos(), Some(7));
    }

    #[test]
    fn collapse_restore_only_at_start() {
        let mark = marked(4);
        let offset = mark.collapse_offset(2..5);
        assert_eq!(offset, Some(2));

        let mut collapsed = marked(2);
        collapsed.restore_collapsed(2, offset.unwrap());
        assert_eq!(collapsed.pos(), Some(4));

        let mut moved = marked(3);
        moved.restore_collapsed(2, offset.unwrap());
        assert_eq!(moved.pos(), Some(3));
    }

    #[test]
    fn collapse_not_recorded_at_start_or_outside() {
        assert_eq!(marked(2).collapse_offset(2..5), None);
        assert_eq!(marked(1).collapse_offset(2..5), None);
        assert_eq!(marked(6).collapse_offset(2..5), None);
        assert_eq!(marked(3).collapse_offset(3..3), None);
    }
}
