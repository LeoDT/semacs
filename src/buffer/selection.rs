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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Activation {
    #[default]
    Inactive,
    Command,
    Pointer,
}

#[derive(Default)]
pub(super) struct TrackedMark {
    position: Option<ByteOffset>,
    activation: Activation,
    revision: u64,
}

impl TrackedMark {
    pub(super) fn position(&self) -> Option<ByteOffset> {
        self.position
    }

    pub(super) fn is_active(&self) -> bool {
        self.activation != Activation::Inactive
    }

    pub(super) fn is_pointer(&self) -> bool {
        self.activation == Activation::Pointer
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn set(&mut self, position: Option<ByteOffset>, active: bool) {
        self.activate(
            position,
            if active {
                Activation::Command
            } else {
                Activation::Inactive
            },
        );
    }

    pub(super) fn set_pointer(&mut self, position: ByteOffset) {
        self.activate(Some(position), Activation::Pointer);
    }

    fn activate(&mut self, position: Option<ByteOffset>, activation: Activation) {
        if (self.position, self.activation) != (position, activation) {
            self.revision = self
                .revision
                .checked_add(1)
                .expect("mark revision exhausted");
            self.position = position;
            self.activation = activation;
        }
    }

    pub(super) fn deactivate(&mut self) {
        self.set(self.position, false);
    }
}
