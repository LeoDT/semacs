//! A buffer-owned section tree and its visible logical lines.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use crate::text::{ByteOffset, Point, Rope};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct Section {
    pub key: String,
    pub parent: Option<usize>,
    pub range: Range<ByteOffset>,
    pub body: ByteOffset,
    pub collapsed: bool,
    /// A diff hunk heading, the only section type Magit pins to the top of the
    /// window after section movement.
    pub hunk: bool,
}

#[derive(Clone, Debug)]
pub struct SectionAnchor {
    ancestry: Vec<String>,
    row: u32,
    column: u32,
}

#[derive(Clone, Debug)]
pub struct Sections {
    nodes: Vec<Section>,
    lines: Arc<[Range<ByteOffset>]>,
    visible: Arc<[Range<ByteOffset>]>,
    hidden: Vec<(Range<ByteOffset>, ByteOffset)>,
    remembered: BTreeMap<String, bool>,
}

impl Sections {
    pub fn new(text: &Rope, nodes: Vec<Section>) -> Result<Self, String> {
        if nodes.is_empty()
            || nodes[0].parent.is_some()
            || nodes[0].range != (ByteOffset(0)..ByteOffset(text.len()))
        {
            return Err("Sections require a root covering the buffer".into());
        }
        let mut keys = std::collections::HashSet::new();
        let mut siblings: Vec<Option<usize>> = vec![None; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            if !keys.insert(&node.key)
                || node.range.start > node.body
                || node.body > node.range.end
                || !text.is_char_boundary(node.range.start.0)
                || !text.is_char_boundary(node.body.0)
                || !text.is_char_boundary(node.range.end.0)
                || (node.range.start.0 != text.len()
                    && text.offset_to_point(node.range.start.0).column != 0)
                || (node.body.0 != text.len() && text.offset_to_point(node.body.0).column != 0)
                || (node.range.end.0 != text.len()
                    && text.offset_to_point(node.range.end.0).column != 0)
            {
                return Err("Invalid section boundaries or duplicate identity".into());
            }
            if index > 0 {
                let parent = node
                    .parent
                    .filter(|parent| *parent < index)
                    .ok_or("Invalid section parent")?;
                let owner = &nodes[parent];
                if node.range.start < owner.body || node.range.end > owner.range.end {
                    return Err("Section is outside its parent body".into());
                }
                if nodes[index - 1].range.start > node.range.start {
                    return Err("Sections must be ordered by heading position".into());
                }
                if let Some(previous) = siblings[parent]
                    && nodes[previous].range.end > node.range.start
                {
                    return Err("Sibling sections overlap".into());
                }
                siblings[parent] = Some(index);
            }
        }
        let lines = (0..=text.max_point().row)
            .map(|row| {
                let start = text.point_to_offset(Point::new(row, 0));
                let end = if row < text.max_point().row {
                    text.point_to_offset(Point::new(row + 1, 0))
                } else {
                    text.len()
                };
                ByteOffset(start)..ByteOffset(end)
            })
            .collect::<Vec<_>>()
            .into();
        let mut result = Self {
            nodes,
            lines,
            visible: Arc::from([]),
            hidden: Vec::new(),
            remembered: BTreeMap::new(),
        };
        result.rebuild();
        Ok(result)
    }

    pub fn nodes(&self) -> &[Section] {
        &self.nodes
    }
    pub fn visible_lines(&self) -> Arc<[Range<ByteOffset>]> {
        self.visible.clone()
    }

    pub fn current(&self, point: ByteOffset) -> usize {
        let mut index = self
            .nodes
            .partition_point(|node| node.range.start <= point)
            .saturating_sub(1);
        while !self.nodes[index].range.contains(&point) {
            let Some(parent) = self.nodes[index].parent else {
                return 0;
            };
            index = parent;
        }
        index
    }

    pub fn toggle(&mut self, index: usize) -> Result<(), String> {
        if index == 0 {
            return Err("The root section cannot be hidden".into());
        }
        let node = self.nodes.get_mut(index).ok_or("No section here")?;
        if node.body == node.range.end {
            return Ok(());
        }
        node.collapsed = !node.collapsed;
        self.remembered.insert(node.key.clone(), node.collapsed);
        self.rebuild();
        Ok(())
    }

    /// Magit `magit-section-cycle-global`: expand every body unless all bodies
    /// are already visible, in which case collapse the top-level sections.
    pub fn cycle_global(&mut self) {
        let children = self.child_indices();
        let top_level = children[0].clone();
        let partially_collapsed = top_level.iter().any(|&index| self.nodes[index].collapsed)
            && top_level.iter().any(|&index| !children[index].is_empty());
        if partially_collapsed
            || top_level
                .iter()
                .any(|&index| self.hidden_body(index, &children))
        {
            for node in &mut self.nodes {
                if node.collapsed {
                    node.collapsed = false;
                    self.remembered.insert(node.key.clone(), false);
                }
            }
        } else {
            for index in top_level {
                let node = &mut self.nodes[index];
                if node.body < node.range.end && !node.collapsed {
                    node.collapsed = true;
                    self.remembered.insert(node.key.clone(), true);
                }
            }
        }
        self.rebuild();
    }

    fn child_indices(&self) -> Vec<Vec<usize>> {
        let mut children = vec![Vec::new(); self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate().skip(1) {
            if let Some(parent) = node.parent {
                children[parent].push(index);
            }
        }
        children
    }

    /// Mirrors `magit-section-hidden-body`: a leaf's body counts only when it has
    /// one, and a section with children defers to them.
    fn hidden_body(&self, index: usize, children: &[Vec<usize>]) -> bool {
        if children[index].is_empty() {
            self.nodes[index].body < self.nodes[index].range.end && self.nodes[index].collapsed
        } else {
            children[index]
                .iter()
                .any(|&child| self.hidden_body(child, children))
        }
    }

    pub fn reveal(&mut self, point: ByteOffset) {
        let mut index = self.nodes[self.current(point)].parent;
        while let Some(parent) = index {
            let node = &mut self.nodes[parent];
            node.collapsed = false;
            self.remembered.insert(node.key.clone(), false);
            index = node.parent;
        }
        self.rebuild();
    }

    pub fn restore(&mut self, previous: &Self) {
        self.remembered = previous.remembered.clone();
        for node in &previous.nodes {
            self.remembered.insert(node.key.clone(), node.collapsed);
        }
        for node in &mut self.nodes {
            if let Some(hidden) = self.remembered.get(&node.key) {
                node.collapsed = *hidden;
            }
        }
        if self.remembered.len() > 4096 {
            let current: std::collections::HashSet<_> =
                self.nodes.iter().map(|node| &node.key).collect();
            self.remembered.retain(|key, _| current.contains(key));
        }
        self.rebuild();
    }

    pub fn collapse_point(&self, point: ByteOffset) -> ByteOffset {
        self.hidden_at(point).map_or(point, |(_, heading)| *heading)
    }

    pub fn motion_point(&self, point: ByteOffset, forward: bool) -> ByteOffset {
        self.hidden_at(point).map_or(point, |(range, _)| {
            if forward {
                range.end
            } else {
                ByteOffset(range.start.0.saturating_sub(1))
            }
        })
    }

    fn hidden_at(&self, point: ByteOffset) -> Option<&(Range<ByteOffset>, ByteOffset)> {
        let index = self.hidden.partition_point(|(range, _)| range.end <= point);
        self.hidden
            .get(index)
            .filter(|(range, _)| range.contains(&point))
    }

    pub fn adjacent_line(&self, point: ByteOffset, forward: bool) -> Option<ByteOffset> {
        let index = self
            .visible
            .partition_point(|line| line.start <= point)
            .checked_sub(1)?;
        let next = if forward {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        self.visible.get(next).map(|line| line.start)
    }

    /// The range of the hunk section whose heading starts at `point`.
    pub fn hunk_at(&self, point: ByteOffset) -> Option<Range<ByteOffset>> {
        let node = &self.nodes[self.current(point)];
        (node.hunk && node.range.start == point).then(|| node.range.clone())
    }

    pub fn navigate(&self, point: ByteOffset, forward: bool) -> Option<ByteOffset> {
        self.nodes
            .iter()
            .skip(1)
            .filter(|node| self.collapse_point(node.range.start) == node.range.start)
            .map(|node| node.range.start)
            .filter(|start| {
                if forward {
                    *start > point
                } else {
                    *start < point
                }
            })
            .reduce(|a, b| if forward { a.min(b) } else { a.max(b) })
    }

    pub fn anchor(&self, text: &Rope, point: ByteOffset) -> SectionAnchor {
        let mut index = self.current(point);
        let position = text.offset_to_point(point.0);
        let start = text.offset_to_point(self.nodes[index].range.start.0);
        let mut ancestry = Vec::new();
        loop {
            let node = &self.nodes[index];
            ancestry.push(node.key.clone());
            let Some(parent) = node.parent else {
                break;
            };
            index = parent;
        }
        SectionAnchor {
            ancestry,
            row: position.row - start.row,
            column: position.column,
        }
    }

    pub fn resolve(&self, text: &Rope, anchor: &SectionAnchor) -> ByteOffset {
        let Some(node) = self
            .nodes
            .iter()
            .find(|node| Some(&node.key) == anchor.ancestry.first())
        else {
            return anchor
                .ancestry
                .iter()
                .skip(1)
                .find_map(|key| self.nodes.iter().find(|node| &node.key == key))
                .map_or(ByteOffset(0), |node| self.collapse_point(node.range.start));
        };
        let start = text.offset_to_point(node.range.start.0).row;
        let last = text.offset_to_point(node.range.end.0.saturating_sub(1)).row;
        let row = start.saturating_add(anchor.row).min(last);
        let mut offset =
            text.point_to_offset(Point::new(row, anchor.column.min(text.line_len(row))));
        while !text.is_char_boundary(offset) {
            offset = offset.saturating_sub(1);
        }
        self.collapse_point(ByteOffset(offset))
    }

    fn rebuild(&mut self) {
        #[cfg(feature = "native-profiler")]
        let _span = crate::preparation_trace::Span::new("section-projection", 0);
        self.hidden.clear();
        for node in self
            .nodes
            .iter()
            .skip(1)
            .filter(|node| node.collapsed && node.body < node.range.end)
        {
            if !self
                .hidden
                .last()
                .is_some_and(|(range, _)| range.contains(&node.range.start))
            {
                self.hidden
                    .push((node.body..node.range.end, node.range.start));
            }
        }
        self.hidden.sort_by_key(|(range, _)| range.start);
        self.visible = self
            .lines
            .iter()
            .filter(|line| self.collapse_point(line.start) == line.start)
            .cloned()
            .collect();
    }
}
