use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::text::Rope;

const CAPACITY: usize = 120;

/// Platform clipboard bridge used by the kill ring (Emacs
/// `interprogram-cut-function` / `interprogram-paste-function`).
pub trait SystemClipboard {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str);
}

#[derive(Default)]
struct State {
    entries: VecDeque<Rope>,
    last_was_kill: bool,
    /// Last non-empty clipboard text written by a kill or imported by a yank
    /// (Emacs `gui--last-selected-text-clipboard`). Empty strings are stored as
    /// `None` to match platform backends that treat empty clipboard text as
    /// absent.
    last_clipboard_text: Option<String>,
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
            last_clipboard_text: None,
        })))
    }

    pub fn latest(&self) -> Option<Rope> {
        self.0.borrow().entries.front().cloned()
    }

    /// Returns the latest kill entry, first importing any unseen non-empty text
    /// from the system clipboard onto the front of the ring (Emacs
    /// `(current-kill 0)` + `interprogram-paste-function`).
    pub fn current_kill(&self, clipboard: Option<&dyn SystemClipboard>) -> Option<Rope> {
        if let Some(clipboard) = clipboard {
            let external = clipboard.read_text().filter(|text| !text.is_empty());
            let mut state = self.0.borrow_mut();
            if external != state.last_clipboard_text {
                state.last_clipboard_text = external.clone();
                if let Some(text) = external {
                    state.entries.push_front(Rope::from(text.as_str()));
                    state.entries.truncate(CAPACITY);
                    state.last_was_kill = false;
                }
            }
            return state.entries.front().cloned();
        }
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

    pub(super) fn record(
        &self,
        mut text: Rope,
        append: bool,
        prepend: bool,
        kill: bool,
        clipboard: Option<&dyn SystemClipboard>,
    ) {
        let exported = {
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
            if clipboard.is_some() {
                let latest = state
                    .entries
                    .front()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                state.last_clipboard_text = (!latest.is_empty()).then(|| latest.clone());
                Some(latest)
            } else {
                None
            }
        };
        if let (Some(clipboard), Some(text)) = (clipboard, exported) {
            clipboard.write_text(&text);
        }
    }
}
