//! Pure split-tree state; view identity, layout constraints, and rendering live
//! in the application shell.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Left/right arrangement (a row).
    Horizontal,
    /// Top/bottom arrangement (a column).
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pane<T> {
    Leaf(T),
    Split {
        axis: Axis,
        first: Box<Pane<T>>,
        second: Box<Pane<T>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Panes<T: Clone + PartialEq> {
    root: Pane<T>,
    selected: T,
}

impl<T: Clone + PartialEq> Panes<T> {
    pub(crate) fn new(initial: T) -> Self {
        Self {
            root: Pane::Leaf(initial.clone()),
            selected: initial,
        }
    }

    pub(crate) fn selected(&self) -> &T {
        &self.selected
    }

    /// Return leaf identities in deterministic depth-first (first, second) order.
    pub(crate) fn leaves(&self) -> Vec<T> {
        let mut leaves = Vec::new();
        collect_leaves(&self.root, &mut leaves);
        leaves
    }

    pub(crate) fn root(&self) -> &Pane<T> {
        &self.root
    }

    pub(crate) fn select(&mut self, identity: &T) -> bool {
        if contains(&self.root, identity) {
            self.selected = identity.clone();
            true
        } else {
            false
        }
    }

    /// Select the next leaf in depth-first visual order, wrapping at the end.
    pub(crate) fn next(&mut self) -> &T {
        let leaves = self.leaves();
        let index = leaves
            .iter()
            .position(|leaf| leaf == &self.selected)
            .unwrap_or(0);
        self.selected = leaves[(index + 1) % leaves.len()].clone();
        &self.selected
    }

    /// Replace the selected leaf by a split, retaining it as the first child.
    pub(crate) fn split(&mut self, new: T, axis: Axis) {
        let selected = &self.selected;
        split_leaf(&mut self.root, selected, new, axis);
    }

    /// Remove the selected leaf and collapse its parent. The final leaf cannot be removed.
    pub(crate) fn close_selected(&mut self) -> bool {
        let before = self.leaves();
        if before.len() <= 1 {
            return false;
        }
        let old_index = before
            .iter()
            .position(|leaf| leaf == &self.selected)
            .unwrap_or(0);
        self.root = remove_leaf(self.root.clone(), &self.selected)
            .expect("selected leaf exists in a non-empty tree");
        let after = self.leaves();
        self.selected = after[old_index.min(after.len() - 1)].clone();
        true
    }

    /// Discard every other leaf and make the selected identity the root leaf.
    pub(crate) fn retain_selected(&mut self) {
        self.root = Pane::Leaf(self.selected.clone());
    }
}

fn collect_leaves<T: Clone>(pane: &Pane<T>, out: &mut Vec<T>) {
    match pane {
        Pane::Leaf(value) => out.push(value.clone()),
        Pane::Split { first, second, .. } => {
            collect_leaves(first, out);
            collect_leaves(second, out);
        }
    }
}

fn contains<T: PartialEq>(pane: &Pane<T>, identity: &T) -> bool {
    match pane {
        Pane::Leaf(value) => value == identity,
        Pane::Split { first, second, .. } => {
            contains(first, identity) || contains(second, identity)
        }
    }
}

fn split_leaf<T: Clone + PartialEq>(pane: &mut Pane<T>, identity: &T, new: T, axis: Axis) -> bool {
    match pane {
        Pane::Leaf(value) if value == identity => {
            let first = Pane::Leaf(value.clone());
            *pane = Pane::Split {
                axis,
                first: Box::new(first),
                second: Box::new(Pane::Leaf(new)),
            };
            true
        }
        Pane::Leaf(_) => false,
        Pane::Split { first, second, .. } => {
            split_leaf(first, identity, new.clone(), axis)
                || split_leaf(second, identity, new, axis)
        }
    }
}

fn remove_leaf<T: Clone + PartialEq>(pane: Pane<T>, identity: &T) -> Option<Pane<T>> {
    match pane {
        Pane::Leaf(value) => (&value != identity).then_some(Pane::Leaf(value)),
        Pane::Split {
            axis,
            first,
            second,
        } => match remove_leaf(*first, identity) {
            None => Some(*second),
            Some(first) => match remove_leaf(*second, identity) {
                None => Some(first),
                Some(second) => Some(Pane::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, Pane, Panes};

    #[test]
    fn nested_splits_preserve_axes_and_depth_first_order() {
        let mut panes = Panes::new(1);
        panes.split(2, Axis::Horizontal);
        panes.select(&2);
        panes.split(3, Axis::Vertical);
        assert_eq!(panes.leaves(), vec![1, 2, 3]);
        assert_eq!(panes.selected(), &2);
        assert!(matches!(
            panes.root(),
            Pane::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));
    }

    #[test]
    fn traversal_wraps_and_selection_rejects_unknown_identity() {
        let mut panes = Panes::new(1);
        panes.split(2, Axis::Horizontal);
        assert_eq!(panes.next(), &2);
        assert_eq!(panes.next(), &1);
        assert!(!panes.select(&99));
        assert_eq!(panes.selected(), &1);
    }

    #[test]
    fn close_collapses_parent_and_refuses_last_leaf() {
        let mut panes = Panes::new(1);
        panes.split(2, Axis::Horizontal);
        panes.select(&2);
        assert!(panes.close_selected());
        assert_eq!(panes.leaves(), vec![1]);
        assert_eq!(panes.selected(), &1);
        assert!(!panes.close_selected());
    }

    #[test]
    fn retain_selected_discards_all_other_branches() {
        let mut panes = Panes::new(1);
        panes.split(2, Axis::Horizontal);
        panes.select(&2);
        panes.split(3, Axis::Vertical);
        panes.select(&3);
        panes.retain_selected();
        assert_eq!(panes.root(), &Pane::Leaf(3));
        assert_eq!(panes.leaves(), vec![3]);
    }
}
