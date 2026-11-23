use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::text::Rope;

const CAPACITY: usize = 120;

#[derive(Default)]
struct State {
    entries: VecDeque<Rope>,
    last_was_kill: bool,
}

#[derive(Clone, Default)]
pub struct KillRing(Rc<RefCell<State>>);

impl KillRing {
    /// Creates an independent copy of the current kill ring entries for a
    /// secondary cursor while leaving the kill-append chain broken.
    pub(super) fn fork(&self) -> Self {
        let state = self.0.borrow();
        Self(Rc::new(RefCell::new(State {
            entries: state.entries.clone(),
            last_was_kill: false,
        })))
    }
    pub fn latest(&self) -> Option<Rope> {
        self.0.borrow().entries.front().cloned()
    }

    pub fn entries(&self) -> Vec<Rope> {
        self.0.borrow().entries.iter().cloned().collect()
    }

    pub(super) fn begin_command(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().last_was_kill)
    }

    pub(super) fn break_chain(&self) {
        self.0.borrow_mut().last_was_kill = false;
    }

    pub(super) fn record(&self, mut text: Rope, append: bool, prepend: bool, kill: bool) {
        let mut state = self.0.borrow_mut();
        if append && let Some(previous) = state.entries.front_mut() {
            if prepend {
                text.append(previous.clone());
                *previous = text;
            } else {
                previous.append(text);
            }
        } else {
            state.entries.push_front(text);
            state.entries.truncate(CAPACITY);
        }
        state.last_was_kill = kill;
    }
}
