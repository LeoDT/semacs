//! Buffer collection, activation / MRU order, and Emacs-style unique names.

use std::collections::HashSet;
use std::ops::{Index, IndexMut};
use std::path::Path;

use gpui_kit::{Entity, Subscription};

use crate::saves::SaveState;
use crate::view::BufferView;

pub(crate) struct BufferSlot {
    pub view: Entity<BufferView>,
    pub _subscription: Subscription,
    pub save: SaveState,
}

impl BufferSlot {
    pub fn new(view: Entity<BufferView>, subscription: Subscription) -> Self {
        Self {
            view,
            _subscription: subscription,
            save: SaveState::default(),
        }
    }
}

/// Owns the buffer collection. `active` is the only buffer that may hold focus;
/// `mru` keeps `other-buffer` and exit prompts in access order.
pub(crate) struct BufferSet {
    slots: Vec<BufferSlot>,
    active: usize,
    mru: Vec<Entity<BufferView>>,
}

impl BufferSet {
    pub fn new(slot: BufferSlot) -> Self {
        let view = slot.view.clone();
        Self {
            slots: vec![slot],
            active: 0,
            mru: vec![view],
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BufferSlot> {
        self.slots.iter()
    }

    pub fn push(&mut self, slot: BufferSlot) -> usize {
        let view = slot.view.clone();
        self.slots.push(slot);
        self.mru.retain(|existing| existing != &view);
        self.mru.insert(0, view);
        self.slots.len() - 1
    }

    /// Removes a slot and keeps `active` on a surviving buffer. The caller
    /// restores input / focus when the active buffer was removed or the set was
    /// emptied.
    pub fn remove(&mut self, index: usize) -> BufferSlot {
        let slot = self.slots.remove(index);
        self.mru.retain(|view| view != &slot.view);
        if index < self.active {
            self.active -= 1;
        } else if index == self.active {
            self.active = self
                .mru
                .first()
                .and_then(|view| self.index_of(view))
                .unwrap_or(0);
        }
        slot
    }

    pub fn index_of(&self, view: &Entity<BufferView>) -> Option<usize> {
        self.slots.iter().position(|slot| &slot.view == view)
    }

    pub fn get(&self, index: usize) -> Option<&BufferSlot> {
        self.slots.get(index)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_view(&self) -> &Entity<BufferView> {
        &self.slots[self.active].view
    }

    pub fn set_active(&mut self, index: usize) {
        debug_assert!(index < self.slots.len());
        self.active = index;
    }

    pub fn touch(&mut self, view: &Entity<BufferView>) {
        self.mru.retain(|existing| existing != view);
        self.mru.insert(0, view.clone());
    }

    pub fn views_in_mru(&self) -> &[Entity<BufferView>] {
        &self.mru
    }

    /// `other-buffer`: the most recently used buffer that is not active.
    pub fn other_view(&self) -> Option<&Entity<BufferView>> {
        let active = self.active_view();
        self.mru.iter().find(|view| *view != active)
    }
}

impl Index<usize> for BufferSet {
    type Output = BufferSlot;

    fn index(&self, index: usize) -> &Self::Output {
        &self.slots[index]
    }
}

impl IndexMut<usize> for BufferSet {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.slots[index]
    }
}

/// Emacs `create-file-buffer` / uniquify baseline: the first buffer keeps the
/// base name, later conflicts get a directory suffix, then `<2>`, `<3>`, ...
pub fn unique_file_name(base: &str, path: &Path, used: &HashSet<String>) -> String {
    let mut components = Vec::new();
    for directory in path.parent().into_iter().flat_map(Path::ancestors) {
        let Some(name) = directory.file_name() else {
            continue;
        };
        components.push(name.to_string_lossy().into_owned());
        let candidate = format!("{base}<{}>", components.join("/"));
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    numeric_name(base, used)
}

pub fn numeric_name(base: &str, used: &HashSet<String>) -> String {
    let mut number = 2;
    loop {
        let candidate = format!("{base}<{number}>");
        if !used.contains(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

pub fn available_name(base: &str, path: Option<&Path>, used: &HashSet<String>) -> String {
    if !used.contains(base) {
        return base.to_owned();
    }
    match path {
        Some(path) => unique_file_name(base, path, used),
        None => numeric_name(base, used),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_free_name_is_kept_and_file_conflicts_use_the_directory() {
        let path = Path::new("/tmp/one/same.txt");
        assert_eq!(
            available_name("same.txt", Some(path), &used(&[])),
            "same.txt"
        );
        assert_eq!(
            available_name("same.txt", Some(path), &used(&["same.txt"])),
            "same.txt<one>"
        );
        assert_eq!(
            available_name(
                "same.txt",
                Some(path),
                &used(&["same.txt", "same.txt<one>"])
            ),
            "same.txt<one/tmp>"
        );
        assert_eq!(
            available_name("*scratch*", None, &used(&["*scratch*"])),
            "*scratch*<2>"
        );
    }
}
