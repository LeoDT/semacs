//! Spike-scale own undo stack (one per editing target).
//!
//! Emacs grouping rules that matter for v1:
//! * consecutive self-inserts coalesce into one undo unit,
//! * consecutive delete-backward / delete-forward coalesce,
//! * any other command (motion, yank, newline, ...) starts a new unit,
//! * undo / redo never coalesce with typing,
//! * undo restores the point the group started from, redo the point it ended at.
//!
//! This exists because the base library only coalesces the platform's own
//! key-insert path: every edit we drive through a public API is recorded with an
//! atomic intent, and `set_selected_range` breaks coalescing, so the base cannot
//! reproduce Emacs grouping for our commands (see EVIDENCE.md, S3).

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Typing,
    DeleteBackward,
    DeleteForward,
    Other,
}

impl Kind {
    fn amalgamates(self) -> bool {
        !matches!(self, Kind::Other)
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    /// Range in the document as it was before this change, UTF-8 bytes.
    pub range: Range<usize>,
    pub old: String,
    pub new: String,
    pub kind: Kind,
    /// Offset the mark had inside a deletion, so undo can restore it when the
    /// mark is still collapsed at `range.start` (Emacs marker adjustment).
    pub mark_collapse: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub changes: Vec<Change>,
    /// Point before the first change of the group (restored by undo).
    pub point_before: usize,
}

impl Group {
    /// Emacs redo leaves point at the start of the group's earliest change,
    /// not at the point the group originally ended on. This differs from undo,
    /// which restores `point_before`.
    pub fn redo_point(&self) -> usize {
        self.changes
            .iter()
            .map(|change| change.range.start)
            .min()
            .unwrap_or(self.point_before)
    }
}

#[derive(Default)]
pub struct UndoStacks {
    groups: Vec<Group>,
    redo: Vec<Group>,
    /// Set by a command that does not edit: the next edit starts a new unit.
    barrier: bool,
    last_kind: Option<Kind>,
    /// History position when the target was last saved.
    saved: Option<(usize, usize)>,
}

pub type SharedUndo = Rc<RefCell<UndoStacks>>;

pub fn new_stack() -> SharedUndo {
    Rc::new(RefCell::new(UndoStacks::default()))
}

impl UndoStacks {
    /// End of a command. Non-editing commands break coalescing, which is how
    /// Emacs keeps "type, move, type" as two units.
    pub fn end_command(&mut self, edited: bool) {
        if !edited {
            self.barrier = true;
        }
    }

    /// Mark the current history position as saved (file written / buffer loaded).
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.position());
    }

    fn position(&self) -> (usize, usize) {
        (
            self.groups.len(),
            self.groups.last().map(|g| g.changes.len()).unwrap_or(0),
        )
    }

    /// Emacs-compatible dirty flag:
    /// * same history position as the save point -> clean,
    /// * undoing past the save point drops the marker, which also reads as clean,
    /// * any new edit after that is dirty.
    pub fn is_dirty(&self) -> bool {
        match self.saved {
            Some((saved_groups, saved_changes)) => {
                if self.groups.len() < saved_groups {
                    return false;
                }
                self.position() != (saved_groups, saved_changes)
            }
            None => true,
        }
    }

    pub fn record(&mut self, change: Change, point_before: usize) {
        if let Some((saved_groups, _)) = self.saved {
            if self.groups.len() < saved_groups {
                // The save marker was dropped by undo; a new edit is dirty.
                self.saved = None;
            }
        }
        let kind = change.kind;
        let coalesce = !self.barrier
            && kind.amalgamates()
            && self.last_kind == Some(kind)
            && self.groups.last().is_some();
        self.redo.clear();
        if coalesce {
            let group = self.groups.last_mut().expect("group");
            group.changes.push(change);
        } else {
            self.groups.push(Group {
                changes: vec![change],
                point_before,
            });
        }
        self.last_kind = Some(kind);
        self.barrier = false;
    }

    pub fn undo_group(&mut self) -> Option<Group> {
        let group = self.groups.pop()?;
        self.redo.push(group.clone());
        self.barrier = true;
        self.last_kind = None;
        Some(group)
    }

    pub fn redo_group(&mut self) -> Option<Group> {
        let group = self.redo.pop()?;
        self.groups.push(group.clone());
        self.barrier = true;
        self.last_kind = None;
        Some(group)
    }

    pub fn depth(&self) -> (usize, usize) {
        (self.groups.len(), self.redo.len())
    }

    pub fn clear(&mut self) {
        self.groups.clear();
        self.saved = None;
        self.redo.clear();
        self.barrier = false;
        self.last_kind = None;
    }
}

thread_local! {
    static REPLAYING: Cell<bool> = const { Cell::new(false) };
    static PENDING_KIND: RefCell<Option<Kind>> = const { RefCell::new(None) };
}

/// True while our own undo/redo replay is applying changes; those must not be
/// recorded as new user edits.
pub fn replaying() -> bool {
    REPLAYING.with(|r| r.get())
}

pub fn set_replaying(value: bool) {
    REPLAYING.with(|r| r.set(value));
}

/// Declare the intent of the edit a command is about to apply. The target
/// adapter records the change with this kind instead of guessing from the range.
pub fn with_command_kind<T>(kind: Kind, f: impl FnOnce() -> T) -> T {
    let previous = PENDING_KIND.with(|p| p.replace(Some(kind)));
    let out = f();
    PENDING_KIND.with(|p| *p.borrow_mut() = previous);
    out
}

pub fn take_pending_kind() -> Option<Kind> {
    PENDING_KIND.with(|p| p.borrow_mut().take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(range: Range<usize>, old: &str, new: &str) -> Change {
        Change {
            range,
            old: old.to_string(),
            new: new.to_string(),
            kind: Kind::Typing,
            mark_collapse: None,
        }
    }

    #[test]
    fn redo_point_is_earliest_change_start() {
        let insert = Group {
            changes: vec![change(0..0, "", "a"), change(1..1, "", "b")],
            point_before: 0,
        };
        assert_eq!(insert.redo_point(), 0);

        let delete = Group {
            changes: vec![change(2..3, "c", ""), change(1..2, "b", ""), change(0..1, "a", "")],
            point_before: 3,
        };
        assert_eq!(delete.redo_point(), 0);

        let insert_mid = Group {
            changes: vec![change(2..2, "", "X")],
            point_before: 2,
        };
        assert_eq!(insert_mid.redo_point(), 2);
    }
}
