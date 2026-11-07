use std::collections::HashSet;
use std::ops::{Index, IndexMut};
use std::path::Path;

use crate::buffer::{Buffer, BufferId};
use crate::saves::SaveState;

pub(crate) struct BufferSlot {
    pub buffer: Buffer,
    pub save: SaveState,
}

impl BufferSlot {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            buffer,
            save: SaveState::default(),
        }
    }

    pub fn id(&self) -> BufferId {
        self.buffer.snapshot_id().buffer
    }
}

/// Owns document state and save lifecycle independently of any editing view.
pub(crate) struct BufferSet {
    slots: Vec<BufferSlot>,
    mru: Vec<BufferId>,
}

impl BufferSet {
    pub fn new(slot: BufferSlot) -> Self {
        let id = slot.id();
        Self {
            slots: vec![slot],
            mru: vec![id],
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
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut BufferSlot> {
        self.slots.iter_mut()
    }

    pub fn push(&mut self, slot: BufferSlot) -> usize {
        let id = slot.id();
        self.slots.push(slot);
        self.touch(id);
        self.slots.len() - 1
    }

    pub fn insert_background(&mut self, slot: BufferSlot) -> usize {
        self.mru.push(slot.id());
        self.slots.push(slot);
        self.slots.len() - 1
    }

    pub fn remove(&mut self, id: BufferId) -> Option<BufferSlot> {
        let index = self.index_of(id)?;
        self.mru.retain(|existing| *existing != id);
        Some(self.slots.remove(index))
    }

    pub fn index_of(&self, id: BufferId) -> Option<usize> {
        self.slots.iter().position(|slot| slot.id() == id)
    }

    pub fn get(&self, index: usize) -> Option<&BufferSlot> {
        self.slots.get(index)
    }

    pub fn touch(&mut self, id: BufferId) {
        if self.index_of(id).is_some() {
            self.mru.retain(|existing| *existing != id);
            self.mru.insert(0, id);
        }
    }

    pub fn bury(&mut self, id: BufferId) {
        if self.index_of(id).is_some() {
            self.mru.retain(|existing| *existing != id);
            self.mru.push(id);
        }
    }

    pub fn ids_in_mru(&self) -> &[BufferId] {
        &self.mru
    }

    pub fn other_buffer(&self, current: BufferId) -> Option<BufferId> {
        self.mru.iter().copied().find(|id| *id != current)
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
