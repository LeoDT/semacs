use std::cell::Cell;

use super::SnapshotId;
use crate::text::ByteOffset;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionId {
    pub(super) snapshot: SnapshotId,
    pub(super) view: u64,
    pub(super) point_revision: u64,
    pub(super) mark_revision: u64,
}

pub(super) struct TrackedPoint {
    offset: Cell<usize>,
    revision: Cell<u64>,
}

impl TrackedPoint {
    pub(super) fn new(offset: usize) -> Self {
        Self {
            offset: Cell::new(offset),
            revision: Cell::new(0),
        }
    }

    pub(super) fn get(&self) -> usize {
        self.offset.get()
    }

    pub(super) fn set(&self, offset: usize) {
        if self.offset.replace(offset) != offset {
            self.revision.set(
                self.revision
                    .get()
                    .checked_add(1)
                    .expect("point revision exhausted"),
            );
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision.get()
    }
}

#[derive(Default)]
pub(super) struct TrackedMark {
    position: Option<ByteOffset>,
    active: bool,
    revision: u64,
}

impl TrackedMark {
    pub(super) fn position(&self) -> Option<ByteOffset> {
        self.position
    }

    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn set(&mut self, position: Option<ByteOffset>, active: bool) {
        if (self.position, self.active) != (position, active) {
            self.revision = self
                .revision
                .checked_add(1)
                .expect("mark revision exhausted");
            self.position = position;
            self.active = active;
        }
    }

    pub(super) fn deactivate(&mut self) {
        self.set(self.position, false);
    }
}
