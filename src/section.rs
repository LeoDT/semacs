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
struct AnchorStep {
    key: String,
    next_siblings: Vec<String>,
    prev_siblings: Vec<String>,
    sibling_index: usize,
    hunk: bool,
}

#[derive(Clone, Debug)]
pub struct SectionAnchor {
    steps: Vec<AnchorStep>,
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
        let at_buffer_end = self.at_content_end(point);
        while !self.nodes[index].range.contains(&point)
            && !(at_buffer_end && self.nodes[index].range.end == point)
        {
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

    pub fn is_range_hidden(&self, range: Range<ByteOffset>) -> bool {
        self.hidden
            .iter()
            .any(|(hidden, _)| ranges_intersect(&range, hidden))
    }

    /// Temporarily expands any collapsed section whose body intersects `range`,
    /// recording newly opened keys in `opened`, and immediately re-collapses
    /// previously recorded keys whose bodies no longer intersect `range`
    /// (matching Emacs `search-invisible = 'open` and `isearch-hide-immediately`).
    pub fn temp_open_for_range(
        &mut self,
        range: Range<ByteOffset>,
        opened: &mut Vec<String>,
    ) -> bool {
        let mut changed = false;
        let mut kept = Vec::with_capacity(opened.len());
        for key in opened.drain(..) {
            if let Some(node) = self.nodes.iter_mut().find(|node| node.key == key) {
                let body = node.body..node.range.end;
                if ranges_intersect(&range, &body) {
                    kept.push(key);
                } else if !node.collapsed {
                    node.collapsed = true;
                    changed = true;
                }
            }
        }
        *opened = kept;
        for node in self.nodes.iter_mut().skip(1) {
            if node.collapsed && node.body < node.range.end {
                let body = node.body..node.range.end;
                if ranges_intersect(&range, &body) {
                    node.collapsed = false;
                    if !opened.iter().any(|key| key == &node.key) {
                        opened.push(node.key.clone());
                    }
                    changed = true;
                }
            }
        }
        if changed {
            self.rebuild();
        }
        changed
    }

    /// Finalizes temporarily opened sections when ending isearch: keeps any
    /// section whose body intersects `keep_range` permanently open (recording it
    /// in `remembered`) and re-collapses all other temporarily opened sections.
    pub fn finalize_temp_opened(
        &mut self,
        keep_range: Option<Range<ByteOffset>>,
        opened: &mut Vec<String>,
    ) -> bool {
        if opened.is_empty() {
            return false;
        }
        let mut changed = false;
        for key in opened.drain(..) {
            if let Some(node) = self.nodes.iter_mut().find(|node| node.key == key) {
                let body = node.body..node.range.end;
                let keep = keep_range
                    .as_ref()
                    .is_some_and(|range| ranges_intersect(range, &body));
                if keep {
                    self.remembered.insert(node.key.clone(), false);
                } else if !node.collapsed {
                    node.collapsed = true;
                    changed = true;
                }
            }
        }
        if changed {
            self.rebuild();
        }
        changed
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
        self.hidden_at(point).map_or(point, |(range, heading)| {
            if forward && self.at_content_end(range.end) {
                *heading
            } else if forward {
                range.end
            } else {
                ByteOffset(range.start.0.saturating_sub(1))
            }
        })
    }

    fn at_content_end(&self, point: ByteOffset) -> bool {
        point == self.nodes[0].range.end
            && self.lines.last().is_some_and(|line| line.start < line.end)
    }

    fn hidden_at(&self, point: ByteOffset) -> Option<&(Range<ByteOffset>, ByteOffset)> {
        let point = if self.at_content_end(point) {
            ByteOffset(point.0.saturating_sub(1))
        } else {
            point
        };
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

    /// Mirrors `magit-section-forward-sibling` / `magit-section-backward-sibling`:
    /// moves to the next/previous visible sibling section, falling back to
    /// `navigate` when there is no sibling in that direction or at root.
    pub fn navigate_sibling(&self, point: ByteOffset, forward: bool) -> Option<ByteOffset> {
        let current = self.current(point);
        if current == 0 {
            return self.navigate(point, forward);
        }
        let parent = self.nodes[current].parent;
        let current_start = self.nodes[current].range.start;
        let sibling = self
            .nodes
            .iter()
            .skip(1)
            .filter(|node| {
                node.parent == parent && self.collapse_point(node.range.start) == node.range.start
            })
            .map(|node| node.range.start)
            .filter(|start| {
                if forward {
                    *start > current_start
                } else {
                    *start < current_start
                }
            })
            .reduce(|a, b| if forward { a.min(b) } else { a.max(b) });
        sibling.or_else(|| {
            if forward {
                let current_end = self.nodes[current].range.end;
                self.nodes
                    .iter()
                    .skip(1)
                    .filter(|node| self.collapse_point(node.range.start) == node.range.start)
                    .map(|node| node.range.start)
                    .filter(|start| *start >= current_end)
                    .reduce(|a, b| a.min(b))
            } else {
                self.navigate(point, false)
            }
        })
    }

    pub fn find_key(&self, key: &str) -> Option<ByteOffset> {
        self.nodes
            .iter()
            .find(|node| node.key == key)
            .map(|node| self.collapse_point(node.range.start))
    }

    pub fn foldable_heading_at(&self, point: ByteOffset) -> Option<usize> {
        let index = self.current(point);
        let node = self.nodes.get(index)?;
        (index > 0
            && node.body < node.range.end
            && point >= node.range.start
            && point < node.body
            && self.collapse_point(point) == point)
            .then_some(index)
    }

    fn depth(&self, mut index: usize) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes[index].parent {
            depth += 1;
            index = parent;
        }
        depth
    }

    fn set_collapsed(&mut self, index: usize, collapsed: bool) {
        let node = &mut self.nodes[index];
        if node.collapsed != collapsed {
            node.collapsed = collapsed;
            self.remembered.insert(node.key.clone(), collapsed);
        }
    }

    fn show_children_1(&mut self, index: usize, depth: Option<usize>, children: &[Vec<usize>]) {
        for &child in &children[index] {
            self.set_collapsed(child, false);
            match depth {
                Some(0) => {
                    if self.nodes[child].body < self.nodes[child].range.end {
                        self.set_collapsed(child, true);
                    }
                }
                Some(d) => self.show_children_1(child, Some(d - 1), children),
                None => self.show_children_1(child, None, children),
            }
        }
    }

    fn show_headings_1(&mut self, index: usize, children: &[Vec<usize>]) {
        for &child in &children[index] {
            self.set_collapsed(child, false);
            if !children[child].is_empty() || self.nodes[child].body == self.nodes[child].range.end
            {
                self.show_headings_1(child, children);
            }
        }
    }

    fn hide_children(&mut self, index: usize, children: &[Vec<usize>]) {
        for &child in &children[index] {
            if self.nodes[child].body < self.nodes[child].range.end {
                self.set_collapsed(child, true);
            }
        }
    }

    /// Mirrors `magit-section-show-level` (1..=4).
    pub fn show_level(&mut self, point: ByteOffset, level: usize) -> ByteOffset {
        let children = self.child_indices();
        let mut s = self.current(point);
        let mut i = self.depth(s);
        while i > level {
            if let Some(parent) = self.nodes[s].parent {
                s = parent;
                i -= 1;
            } else {
                break;
            }
        }
        if i < level {
            if s != 0 {
                self.set_collapsed(s, false);
            }
            self.show_children_1(s, Some(level - i - 1), &children);
        } else if s != 0 && self.nodes[s].body < self.nodes[s].range.end {
            self.set_collapsed(s, true);
        }
        self.rebuild();
        self.collapse_point(self.nodes[s].range.start)
    }

    /// Mirrors `magit-section-show-level` with negative arg (`M-1`..`M-4`).
    pub fn show_level_all(&mut self, point: ByteOffset, level: usize) -> ByteOffset {
        let children = self.child_indices();
        let mut s = self.current(point);
        while self.depth(s) > level {
            let Some(parent) = self.nodes[s].parent else {
                break;
            };
            s = parent;
        }
        self.show_children_1(0, Some(level.saturating_sub(1)), &children);
        self.rebuild();
        self.collapse_point(self.nodes[s].range.start)
    }

    /// Mirrors `magit-section-cycle` (`C-c TAB` / `C-<tab>`).
    pub fn cycle(&mut self, point: ByteOffset) {
        let current = self.current(point);
        if current == 0 {
            self.cycle_global();
            return;
        }
        let children = self.child_indices();
        if self.nodes[current].collapsed {
            self.set_collapsed(current, false);
            self.hide_children(current, &children);
        } else {
            let ch = &children[current];
            if ch.iter().any(|&c| self.nodes[c].collapsed)
                && ch.iter().any(|&c| !children[c].is_empty())
            {
                self.set_collapsed(current, false);
                self.show_headings_1(current, &children);
            } else if ch.iter().any(|&c| self.hidden_body(c, &children)) {
                self.set_collapsed(current, false);
                self.show_children_1(current, None, &children);
            } else if self.nodes[current].body < self.nodes[current].range.end {
                self.set_collapsed(current, true);
            }
        }
        self.rebuild();
    }

    /// Mirrors `magit-section-cycle-diffs` (`M-<tab>`).
    pub fn cycle_diffs(&mut self, status_mode: bool) {
        let children = self.child_indices();
        let sections: Vec<usize> = if status_mode {
            let mut list = Vec::new();
            for key in ["staged", "unstaged"] {
                if let Some(group) = self.nodes.iter().position(|node| node.key == key) {
                    if self.nodes[group].collapsed {
                        self.set_collapsed(group, false);
                    }
                    list.extend_from_slice(&children[group]);
                }
            }
            list
        } else {
            children[0]
                .iter()
                .copied()
                .filter(|&index| self.nodes[index].key.starts_with("file/"))
                .collect()
        };
        if sections.is_empty() {
            self.rebuild();
            return;
        }
        if sections.iter().any(|&s| self.nodes[s].collapsed) {
            for &s in &sections {
                self.set_collapsed(s, false);
                self.hide_children(s, &children);
            }
        } else {
            let grandchildren: Vec<usize> = sections
                .iter()
                .flat_map(|&s| children[s].iter().copied())
                .collect();
            if grandchildren.iter().any(|&c| self.nodes[c].collapsed)
                && grandchildren.iter().any(|&c| !children[c].is_empty())
            {
                for &s in &sections {
                    self.set_collapsed(s, false);
                    self.show_headings_1(s, &children);
                }
            } else if grandchildren.iter().any(|&c| self.hidden_body(c, &children)) {
                for &s in &sections {
                    self.set_collapsed(s, false);
                    self.show_children_1(s, None, &children);
                }
            } else {
                for &s in &sections {
                    if self.nodes[s].body < self.nodes[s].range.end {
                        self.set_collapsed(s, true);
                    }
                }
            }
        }
        self.rebuild();
    }

    pub fn anchor(&self, text: &Rope, point: ByteOffset) -> SectionAnchor {
        let children = self.child_indices();
        let mut index = self.current(point);
        let position = text.offset_to_point(point.0);
        let start = text.offset_to_point(self.nodes[index].range.start.0);
        let mut steps = Vec::new();
        loop {
            let node = &self.nodes[index];
            let (prev_siblings, next_siblings, sibling_index) = match node.parent {
                Some(parent) => {
                    let siblings = &children[parent];
                    let pos = siblings
                        .iter()
                        .position(|&child| child == index)
                        .unwrap_or(0);
                    let prev = siblings[..pos]
                        .iter()
                        .rev()
                        .map(|&child| self.nodes[child].key.clone())
                        .collect();
                    let next = siblings[pos + 1..]
                        .iter()
                        .map(|&child| self.nodes[child].key.clone())
                        .collect();
                    (prev, next, pos)
                }
                None => (Vec::new(), Vec::new(), 0),
            };
            steps.push(AnchorStep {
                key: node.key.clone(),
                next_siblings,
                prev_siblings,
                sibling_index,
                hunk: node.hunk,
            });
            let Some(parent) = node.parent else {
                break;
            };
            index = parent;
        }
        SectionAnchor {
            steps,
            row: position.row - start.row,
            column: position.column,
        }
    }

    pub fn resolve(&self, text: &Rope, anchor: &SectionAnchor) -> ByteOffset {
        let find_node = |key: &str| self.nodes.iter().position(|node| node.key == key);
        if let Some(first) = anchor.steps.first()
            && let Some(index) = find_node(&first.key)
        {
            let node = &self.nodes[index];
            let start = text.offset_to_point(node.range.start.0).row;
            let last = text.offset_to_point(node.range.end.0.saturating_sub(1)).row;
            let row = start.saturating_add(anchor.row).min(last);
            let mut offset =
                text.point_to_offset(Point::new(row, anchor.column.min(text.line_len(row))));
            while !text.is_char_boundary(offset) {
                offset = offset.saturating_sub(1);
            }
            return self.collapse_point(ByteOffset(offset));
        }
        if let Some(first) = anchor.steps.first()
            && first.hunk
            && let Some(parent_step) = anchor.steps.get(1)
            && let Some(parent_idx) = find_node(&parent_step.key)
        {
            let children = self.child_indices();
            if let Some(&child) = children[parent_idx]
                .get(first.sibling_index)
                .or_else(|| children[parent_idx].last())
            {
                return self.collapse_point(self.nodes[child].range.start);
            }
        }
        for (i, step) in anchor.steps.iter().enumerate() {
            let paired = match step.key.as_str() {
                "staged" => Some("unstaged"),
                "unstaged" => Some("staged"),
                "unpushed-upstream" => Some("unpulled-upstream"),
                "unpulled-upstream" => Some("unpushed-upstream"),
                "unpushed-pushremote" => Some("unpulled-pushremote"),
                "unpulled-pushremote" => Some("unpushed-pushremote"),
                _ => None,
            };
            if let Some(paired_idx) = paired.and_then(find_node) {
                return self.collapse_point(self.nodes[paired_idx].range.start);
            }
            if let Some(next_idx) = step.next_siblings.iter().find_map(|key| find_node(key)) {
                return self.collapse_point(self.nodes[next_idx].range.start);
            }
            if let Some(prev_idx) = step.prev_siblings.iter().find_map(|key| find_node(key)) {
                return self.collapse_point(self.nodes[prev_idx].range.start);
            }
            if let Some(parent_idx) = anchor
                .steps
                .get(i + 1)
                .and_then(|parent| find_node(&parent.key))
            {
                return self.collapse_point(self.nodes[parent_idx].range.start);
            }
        }
        ByteOffset(0)
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

fn ranges_intersect(a: &Range<ByteOffset>, b: &Range<ByteOffset>) -> bool {
    if b.start >= b.end {
        return false;
    }
    if a.start == a.end {
        a.start >= b.start && a.start < b.end
    } else {
        a.start < b.end && a.end > b.start
    }
}
