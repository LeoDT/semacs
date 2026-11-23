//! Emacs `multiple-cursors`-style multi-cursor state, management commands, and
//! synchronized editing/motion execution.

use std::ops::Range;

use crate::buffer::{BatchEdit, EditError, EditGroup, PointMarker};
use crate::command::Command;
use crate::input::{InputUpdate, SessionToken};
use crate::text::{ByteOffset, OffsetUtf16, Point, Rope};

use super::{
    CommandContext, CommandOutcome, EditTarget, KillRing, RouteError, SystemClipboard, column,
    indent, kill, motion,
};

pub(super) struct FakeCursor {
    point: PointMarker,
    mark: Option<PointMarker>,
    mark_active: bool,
    goal_column: Option<usize>,
    kill_ring: KillRing,
}

impl FakeCursor {
    pub(super) fn point(&self) -> ByteOffset {
        self.point.position()
    }

    pub(super) fn mark(&self) -> Option<ByteOffset> {
        self.mark.as_ref().map(PointMarker::position)
    }

    pub(super) fn region(&self) -> Option<Range<ByteOffset>> {
        let point = self.point();
        let mark = self.mark().filter(|m| self.mark_active && *m != point)?;
        Some(point.min(mark)..point.max(mark))
    }

    fn span_start(&self) -> ByteOffset {
        self.region().map_or_else(|| self.point(), |r| r.start)
    }

    fn span_end(&self) -> ByteOffset {
        self.region().map_or_else(|| self.point(), |r| r.end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorId {
    Primary,
    Fake(usize),
}

struct PlannedEdit {
    cursors: Vec<CursorId>,
    range: Range<ByteOffset>,
    text: String,
}

fn next_char(text: &Rope, point: ByteOffset) -> Option<ByteOffset> {
    text.chars_at(point.0)
        .next()
        .map(|ch| ByteOffset(point.0 + ch.len_utf8()))
}

fn prev_char(text: &Rope, point: ByteOffset) -> Option<ByteOffset> {
    text.reversed_chars_at(point.0)
        .next()
        .map(|ch| ByteOffset(point.0 - ch.len_utf8()))
}

fn ranges_overlap(a: &Range<ByteOffset>, b: &Range<ByteOffset>) -> bool {
    a.start < b.end && b.start < a.end
}

fn merge_overlapping_edits(
    mut raw: Vec<(CursorId, Range<ByteOffset>, String)>,
) -> Vec<PlannedEdit> {
    raw.sort_by_key(|(_, range, _)| (range.start, range.end));
    let mut merged: Vec<PlannedEdit> = Vec::with_capacity(raw.len());
    for (cursor, range, text) in raw {
        if let Some(last) = merged.last_mut()
            && range.start < last.range.end
        {
            last.range.end = last.range.end.max(range.end);
            if !text.is_empty() && last.text.is_empty() {
                last.text = text;
            }
            last.cursors.push(cursor);
            continue;
        }
        if let Some(last) = merged.last_mut()
            && range.start == last.range.start
            && range.end == last.range.end
        {
            if !text.is_empty() && last.text.is_empty() {
                last.text = text;
            }
            last.cursors.push(cursor);
            continue;
        }
        merged.push(PlannedEdit {
            cursors: vec![cursor],
            range,
            text,
        });
    }
    merged
}

impl EditTarget {
    pub fn cursor_count(&self) -> usize {
        1 + self.fake_cursors.len()
    }

    pub fn has_multiple_cursors(&self) -> bool {
        !self.fake_cursors.is_empty()
    }

    pub fn secondary_cursors(&self) -> Vec<ByteOffset> {
        self.fake_cursors.iter().map(FakeCursor::point).collect()
    }

    pub fn secondary_regions(&self) -> Vec<Range<ByteOffset>> {
        self.fake_cursors
            .iter()
            .filter_map(FakeCursor::region)
            .collect()
    }

    pub(crate) fn exit_multiple_cursors(&mut self) {
        self.fake_cursors.clear();
    }

    pub(super) fn allows_multiple_cursors(&self) -> Result<(), RouteError> {
        self.buffer.check_editable()?;
        if self.field.is_some()
            || self.buffer.read_only_prefix() > ByteOffset(0)
            || self.buffer.sections().is_some()
        {
            return Err(RouteError::MultipleCursorsUnsupported);
        }
        Ok(())
    }

    pub(super) fn dedup_cursors(&mut self) {
        if self.fake_cursors.is_empty() {
            return;
        }
        let primary = self.buffer.point();
        let mut seen = Vec::with_capacity(self.fake_cursors.len() + 1);
        seen.push(primary);
        self.fake_cursors.retain(|cursor| {
            let pos = cursor.point();
            if seen.contains(&pos) {
                false
            } else {
                seen.push(pos);
                true
            }
        });
    }

    fn create_fake_cursor(
        &self,
        point: ByteOffset,
        mark: Option<ByteOffset>,
        mark_active: bool,
        goal_column: Option<usize>,
    ) -> Result<FakeCursor, EditError> {
        let point = self.buffer.point_marker_at(point)?;
        let mark = match mark {
            Some(offset) => Some(self.buffer.point_marker_at(offset)?),
            None => None,
        };
        Ok(FakeCursor {
            point,
            mark,
            mark_active,
            goal_column,
            kill_ring: self.kill_ring.fork(),
        })
    }

    fn has_cursor_at(&self, offset: ByteOffset) -> bool {
        self.buffer.point() == offset
            || self
                .fake_cursors
                .iter()
                .any(|cursor| cursor.point() == offset)
    }

    fn has_region_overlapping(&self, candidate: &Range<ByteOffset>) -> bool {
        if let Some(primary) = self.buffer.region()
            && ranges_overlap(&primary, candidate)
        {
            return true;
        }
        self.fake_cursors.iter().any(|cursor| {
            cursor
                .region()
                .is_some_and(|region| ranges_overlap(&region, candidate))
        })
    }

    fn cursor_ids(&self) -> Vec<CursorId> {
        let mut ids = Vec::with_capacity(1 + self.fake_cursors.len());
        ids.push(CursorId::Primary);
        for index in 0..self.fake_cursors.len() {
            ids.push(CursorId::Fake(index));
        }
        ids
    }

    fn cursor_point(&self, id: CursorId) -> ByteOffset {
        match id {
            CursorId::Primary => self.buffer.point(),
            CursorId::Fake(index) => self.fake_cursors[index].point(),
        }
    }

    fn cursor_mark(&self, id: CursorId) -> Option<ByteOffset> {
        match id {
            CursorId::Primary => self.buffer.mark(),
            CursorId::Fake(index) => self.fake_cursors[index].mark(),
        }
    }

    fn cursor_region(&self, id: CursorId) -> Option<Range<ByteOffset>> {
        match id {
            CursorId::Primary => self.buffer.region(),
            CursorId::Fake(index) => self.fake_cursors[index].region(),
        }
    }

    fn set_cursor_point(&mut self, id: CursorId, offset: ByteOffset) -> Result<(), EditError> {
        match id {
            CursorId::Primary => self.buffer.set_point(offset),
            CursorId::Fake(index) => self
                .buffer
                .set_marker(&self.fake_cursors[index].point, offset),
        }
    }

    fn set_cursor_mark(
        &mut self,
        id: CursorId,
        offset: ByteOffset,
        active: bool,
    ) -> Result<(), EditError> {
        match id {
            CursorId::Primary => {
                self.buffer.set_mark(offset)?;
                if !active {
                    self.buffer.deactivate_mark();
                }
                Ok(())
            }
            CursorId::Fake(index) => {
                let marker = self.buffer.point_marker_at(offset)?;
                self.fake_cursors[index].mark = Some(marker);
                self.fake_cursors[index].mark_active = active;
                Ok(())
            }
        }
    }

    fn deactivate_all_marks(&mut self) {
        self.buffer.deactivate_mark();
        for cursor in &mut self.fake_cursors {
            cursor.mark_active = false;
        }
    }

    fn any_mark_active(&self) -> bool {
        self.buffer.mark_active() || self.fake_cursors.iter().any(|cursor| cursor.mark_active)
    }

    pub(super) fn break_all_kill_chains(&mut self) {
        self.kill_ring.break_chain();
        for cursor in &self.fake_cursors {
            cursor.kill_ring.break_chain();
        }
    }

    pub(super) fn clear_all_goal_columns(&mut self) {
        self.goal_column = None;
        for cursor in &mut self.fake_cursors {
            cursor.goal_column = None;
        }
    }

    pub(super) fn execute_mc_management(
        &mut self,
        command: Command,
    ) -> Result<CommandOutcome, RouteError> {
        self.allows_multiple_cursors()?;
        match command {
            Command::McMarkNextLikeThis => self.mc_mark_like_this(true),
            Command::McMarkPreviousLikeThis => self.mc_mark_like_this(false),
            Command::McSkipToNextLikeThis => self.mc_skip_like_this(true),
            Command::McSkipToPreviousLikeThis => self.mc_skip_like_this(false),
            Command::McUnmarkNextLikeThis => self.mc_unmark_like_this(true),
            Command::McUnmarkPreviousLikeThis => self.mc_unmark_like_this(false),
            Command::McMarkAllLikeThis => self.mc_mark_all_like_this(),
            Command::McEditLines => self.mc_edit_lines(),
            _ => Err(RouteError::Unavailable(command)),
        }
    }

    fn mc_mark_like_this(&mut self, forward: bool) -> Result<CommandOutcome, RouteError> {
        if let Some(region) = self.buffer.region() {
            let primary_point = self.buffer.point();
            let point_at_end = primary_point == region.end;
            let text = self.buffer.text().to_string();
            let needle = &text[region.start.0..region.end.0];
            let found = if forward {
                let mut search_from = region.end.0;
                for cursor in &self.fake_cursors {
                    search_from = search_from.max(cursor.span_end().0);
                }
                self.find_next_match(&text, needle, search_from, point_at_end)
            } else {
                let mut search_limit = region.start.0;
                for cursor in &self.fake_cursors {
                    search_limit = search_limit.min(cursor.span_start().0);
                }
                self.find_prev_match(&text, needle, search_limit, point_at_end)
            };
            let Some(matched) = found else {
                return Ok(CommandOutcome::message("No more matches"));
            };
            let (pt, mk) = if point_at_end {
                (matched.end, matched.start)
            } else {
                (matched.start, matched.end)
            };
            let fake = self.create_fake_cursor(pt, Some(mk), true, None)?;
            self.fake_cursors.push(fake);
            self.dedup_cursors();
            return Ok(CommandOutcome {
                reveal_point: true,
                ..CommandOutcome::default()
            });
        }

        // No active region: align with upstream `mc/mark-next-like-this` /
        // `mc/mark-previous-like-this` creating a cursor on the next/previous line
        // at the same column.
        let text = self.buffer.text();
        let (furthest_point, furthest_goal) = {
            let mut best = (self.buffer.point(), self.goal_column);
            for cursor in &self.fake_cursors {
                let pt = cursor.point();
                if (forward && pt > best.0) || (!forward && pt < best.0) {
                    best = (pt, cursor.goal_column);
                }
            }
            best
        };
        let row = text.offset_to_point(furthest_point.0).row;
        let line = Self::line_range(&text, row);
        let col =
            furthest_goal.unwrap_or_else(|| column::column(&text, line.start.0..furthest_point.0));
        if self.goal_column.is_none() && furthest_point == self.buffer.point() {
            self.goal_column = Some(col);
        }
        let target_row = if forward {
            if row >= text.max_point().row {
                return Ok(CommandOutcome::message("End of buffer"));
            }
            row + 1
        } else {
            let Some(prev) = row.checked_sub(1) else {
                return Ok(CommandOutcome::message("Beginning of buffer"));
            };
            prev
        };
        let target_line = Self::line_range(&text, target_row);
        let destination = column::offset(&text, target_line.start.0..target_line.end.0, col);
        if self.has_cursor_at(destination) {
            return Ok(CommandOutcome::default());
        }
        let fake = self.create_fake_cursor(destination, None, false, Some(col))?;
        self.fake_cursors.push(fake);
        self.dedup_cursors();
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    fn find_next_match(
        &self,
        text: &str,
        needle: &str,
        mut from: usize,
        point_at_end: bool,
    ) -> Option<Range<ByteOffset>> {
        if needle.is_empty() {
            return None;
        }
        while from <= text.len() {
            let rel = text[from..].find(needle)?;
            let start = from + rel;
            let end = start + needle.len();
            let candidate = ByteOffset(start)..ByteOffset(end);
            let new_point = if point_at_end {
                candidate.end
            } else {
                candidate.start
            };
            if !self.has_region_overlapping(&candidate) && !self.has_cursor_at(new_point) {
                return Some(candidate);
            }
            from = end;
        }
        None
    }

    fn find_prev_match(
        &self,
        text: &str,
        needle: &str,
        mut limit: usize,
        point_at_end: bool,
    ) -> Option<Range<ByteOffset>> {
        if needle.is_empty() {
            return None;
        }
        while limit > 0 {
            let start = text[..limit].rfind(needle)?;
            let end = start + needle.len();
            let candidate = ByteOffset(start)..ByteOffset(end);
            let new_point = if point_at_end {
                candidate.end
            } else {
                candidate.start
            };
            if !self.has_region_overlapping(&candidate) && !self.has_cursor_at(new_point) {
                return Some(candidate);
            }
            limit = start;
        }
        None
    }

    fn furthest_fake_in_direction(&self, forward: bool) -> Option<usize> {
        let primary = self.buffer.point();
        let mut best: Option<(usize, ByteOffset)> = None;
        for (index, cursor) in self.fake_cursors.iter().enumerate() {
            let pt = cursor.point();
            if forward {
                if pt > primary && best.is_none_or(|(_, best_pt)| pt > best_pt) {
                    best = Some((index, pt));
                }
            } else if pt < primary && best.is_none_or(|(_, best_pt)| pt < best_pt) {
                best = Some((index, pt));
            }
        }
        best.map(|(index, _)| index)
    }

    fn mc_skip_like_this(&mut self, forward: bool) -> Result<CommandOutcome, RouteError> {
        let Some(region) = self.buffer.region() else {
            return Ok(CommandOutcome::message("Mark a region first"));
        };
        let Some(remove_index) = self.furthest_fake_in_direction(forward) else {
            return Ok(CommandOutcome::message("No cursors to skip"));
        };
        let point_at_end = self.buffer.point() == region.end;
        let text = self.buffer.text().to_string();
        let needle = &text[region.start.0..region.end.0];
        let found = if forward {
            self.find_next_match(
                &text,
                needle,
                self.fake_cursors[remove_index].span_end().0,
                point_at_end,
            )
        } else {
            self.find_prev_match(
                &text,
                needle,
                self.fake_cursors[remove_index].span_start().0,
                point_at_end,
            )
        };
        let Some(matched) = found else {
            return Ok(CommandOutcome::message("No more matches"));
        };
        self.fake_cursors.remove(remove_index);
        let (pt, mk) = if point_at_end {
            (matched.end, matched.start)
        } else {
            (matched.start, matched.end)
        };
        let fake = self.create_fake_cursor(pt, Some(mk), true, None)?;
        self.fake_cursors.push(fake);
        self.dedup_cursors();
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    fn mc_unmark_like_this(&mut self, forward: bool) -> Result<CommandOutcome, RouteError> {
        let Some(index) = self.furthest_fake_in_direction(forward) else {
            return Ok(CommandOutcome::message("No cursors to unmark"));
        };
        self.fake_cursors.remove(index);
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    fn mc_mark_all_like_this(&mut self) -> Result<CommandOutcome, RouteError> {
        let Some(region) = self.buffer.region() else {
            return Ok(CommandOutcome::message("Mark a region first"));
        };
        let point_at_end = self.buffer.point() == region.end;
        let text = self.buffer.text().to_string();
        let needle = &text[region.start.0..region.end.0];
        self.fake_cursors.clear();
        let mut from = 0;
        let mut added = 0usize;
        while from <= text.len() {
            let Some(rel) = text[from..].find(needle) else {
                break;
            };
            let start = from + rel;
            let end = start + needle.len();
            let candidate = ByteOffset(start)..ByteOffset(end);
            if ranges_overlap(&region, &candidate) {
                from = region.end.0;
                continue;
            }
            from = end;
            let (pt, mk) = if point_at_end {
                (candidate.end, candidate.start)
            } else {
                (candidate.start, candidate.end)
            };
            let fake = self.create_fake_cursor(pt, Some(mk), true, None)?;
            self.fake_cursors.push(fake);
            added += 1;
        }
        self.dedup_cursors();
        if added == 0 {
            return Ok(CommandOutcome::message("No other matches"));
        }
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    fn mc_edit_lines(&mut self) -> Result<CommandOutcome, RouteError> {
        let Some(mark) = self.buffer.mark().filter(|_| self.buffer.mark_active()) else {
            return Ok(CommandOutcome::message("Mark a region first"));
        };
        let text = self.buffer.text();
        let point = self.buffer.point();
        let start = point.min(mark);
        let end = point.max(mark);
        let start_row = text.offset_to_point(start.0).row;
        let end_pt = text.offset_to_point(end.0);
        let end_row = if end_pt.column == 0 && end_pt.row > start_row {
            end_pt.row - 1
        } else {
            end_pt.row
        };
        let point_row = text.offset_to_point(point.0).row;
        let point_line = Self::line_range(&text, point_row);
        let col = column::column(&text, point_line.start.0..point.0);

        self.fake_cursors.clear();
        self.buffer.deactivate_mark();

        let primary_row = point_row.clamp(start_row, end_row);
        let primary_line = Self::line_range(&text, primary_row);
        let primary_dest = column::offset(&text, primary_line.start.0..primary_line.end.0, col);
        self.buffer.set_point(primary_dest)?;

        for row in start_row..=end_row {
            if row == primary_row {
                continue;
            }
            let line = Self::line_range(&text, row);
            let dest = column::offset(&text, line.start.0..line.end.0, col);
            let fake = self.create_fake_cursor(dest, None, false, Some(col))?;
            self.fake_cursors.push(fake);
        }
        self.dedup_cursors();
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    /// Upstream `mc/add-cursor-on-click` (`C-S-<mouse-1>`):
    /// Clicking an existing fake cursor removes it; clicking a new offset turns
    /// the current primary cursor into a fake cursor and moves primary to the
    /// clicked offset.
    pub fn toggle_cursor_at_pointer(
        &mut self,
        token: SessionToken,
        point: ByteOffset,
    ) -> Result<(), RouteError> {
        self.input.guard_command(&self.buffer, token)?;
        self.allows_multiple_cursors()?;
        if !self.buffer.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange.into());
        }
        if let Some(index) = self.fake_cursors.iter().position(|c| c.point() == point) {
            self.fake_cursors.remove(index);
        } else if point != self.buffer.point() {
            let prev_point = self.buffer.point();
            let prev_mark = self.buffer.mark();
            let prev_active = self.buffer.mark_active();
            let fake = self.create_fake_cursor(prev_point, prev_mark, prev_active, None)?;
            self.fake_cursors.push(fake);
            self.buffer.set_point(point)?;
            self.buffer.deactivate_mark();
        }
        self.dedup_cursors();
        self.buffer.break_history_group();
        self.keys.clear();
        self.interrupt_command_chain();
        Ok(())
    }

    pub(super) fn execute_mc_command(
        &mut self,
        token: SessionToken,
        command: Command,
        context: CommandContext<'_>,
        primary_append_kill: bool,
    ) -> Result<CommandOutcome, RouteError> {
        let fake_append_kills: Vec<bool> = self
            .fake_cursors
            .iter()
            .map(|cursor| cursor.kill_ring.begin_command())
            .collect();

        match command {
            // Upstream `mc/keymap` binds RET to exit `multiple-cursors-mode`.
            Command::Newline => {
                self.exit_multiple_cursors();
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            // Upstream `mc/keyboard-quit`: deactivate all regions first if any is
            // active; otherwise exit multiple-cursors mode.
            Command::KeyboardQuit => {
                if self.any_mark_active() {
                    self.deactivate_all_marks();
                } else {
                    self.exit_multiple_cursors();
                }
                Ok(CommandOutcome {
                    quit: true,
                    ..CommandOutcome::message("Quit")
                })
            }
            Command::KeyboardEscapeQuit => {
                self.deactivate_all_marks();
                self.exit_multiple_cursors();
                Ok(CommandOutcome {
                    quit: true,
                    ..CommandOutcome::default()
                })
            }
            Command::ForwardChar | Command::BackwardChar => {
                let forward = command == Command::ForwardChar;
                let text = self.buffer.text();
                let mut moved = false;
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    let dest = if forward {
                        next_char(&text, pt)
                    } else {
                        prev_char(&text, pt)
                    };
                    if let Some(dest) = dest {
                        self.set_cursor_point(id, dest)?;
                        moved = true;
                    }
                }
                self.dedup_cursors();
                if !moved {
                    return Err(if forward {
                        RouteError::EndOfBuffer
                    } else {
                        RouteError::BeginningOfBuffer
                    });
                }
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            Command::MoveBeginningOfLine | Command::MoveEndOfLine => {
                let text = self.buffer.text();
                let to_end = command == Command::MoveEndOfLine;
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    let row = text.offset_to_point(pt.0).row;
                    let dest = if to_end {
                        ByteOffset(text.point_to_offset(Point::new(row, text.line_len(row))))
                    } else {
                        ByteOffset(text.point_to_offset(Point::new(row, 0)))
                    };
                    self.set_cursor_point(id, dest)?;
                }
                self.dedup_cursors();
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            Command::ForwardWord | Command::BackwardWord => {
                let text = self.buffer.text();
                let forward = command == Command::ForwardWord;
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    let dest = motion::word(&text, pt, forward);
                    self.set_cursor_point(id, dest)?;
                }
                self.dedup_cursors();
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            Command::ForwardParagraph | Command::BackwardParagraph => {
                let text = self.buffer.text();
                let forward = command == Command::ForwardParagraph;
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    let dest = motion::paragraph(&text, pt, forward);
                    self.set_cursor_point(id, dest)?;
                }
                self.dedup_cursors();
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            Command::NextLine | Command::PreviousLine => {
                let text = self.buffer.text();
                let forward = command == Command::NextLine;
                let mut moves = Vec::with_capacity(self.cursor_count());
                let mut moved = false;
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    let mut goal = match id {
                        CursorId::Primary => self.goal_column,
                        CursorId::Fake(i) => self.fake_cursors[i].goal_column,
                    };
                    let dest = self.vertical_destination_for(
                        &text,
                        pt,
                        &mut goal,
                        forward,
                        context.viewport,
                    )?;
                    let target = if let Some(dest) = dest {
                        moved = true;
                        dest
                    } else {
                        let edge = ByteOffset(if forward { text.len() } else { 0 });
                        if pt != edge {
                            moved = true;
                        }
                        edge
                    };
                    moves.push((id, goal, target));
                }
                for (id, goal, target) in moves {
                    match id {
                        CursorId::Primary => self.goal_column = goal,
                        CursorId::Fake(i) => self.fake_cursors[i].goal_column = goal,
                    }
                    self.set_cursor_point(id, target)?;
                }
                self.dedup_cursors();
                if !moved {
                    return Err(if forward {
                        RouteError::EndOfBuffer
                    } else {
                        RouteError::BeginningOfBuffer
                    });
                }
                Ok(CommandOutcome {
                    reveal_point: true,
                    ..CommandOutcome::default()
                })
            }
            Command::BeginningOfBuffer | Command::EndOfBuffer => {
                self.exit_multiple_cursors();
                self.execute_single_command(token, command, context, primary_append_kill)
            }
            Command::ScrollUp | Command::ScrollDown | Command::Recenter => {
                let outcome =
                    self.execute_single_command(token, command, context, primary_append_kill)?;
                self.dedup_cursors();
                Ok(outcome)
            }
            Command::SetMark => {
                if self.last_command == Some(Command::SetMark) && self.any_mark_active() {
                    self.deactivate_all_marks();
                    return Ok(CommandOutcome::message("Mark deactivated"));
                }
                for id in self.cursor_ids() {
                    let pt = self.cursor_point(id);
                    self.set_cursor_mark(id, pt, true)?;
                }
                Ok(CommandOutcome::message("Mark set"))
            }
            Command::DeleteChar | Command::DeleteForwardChar | Command::DeleteBackwardChar => {
                self.execute_mc_delete(token, command)
            }
            Command::ElectricNewline => {
                let raw: Vec<_> = self
                    .cursor_ids()
                    .into_iter()
                    .map(|id| {
                        let pt = self.cursor_point(id);
                        (id, pt..pt, "\n".to_owned())
                    })
                    .collect();
                self.apply_mc_edits(token, raw, EditGroup::Isolated)
            }
            Command::Indent => {
                let text = self.buffer.text();
                let raw: Vec<_> = self
                    .cursor_ids()
                    .into_iter()
                    .map(|id| {
                        let pt = self.cursor_point(id);
                        let (range, inserted) = indent::tab(&text, pt, ByteOffset(0));
                        (id, range, inserted)
                    })
                    .collect();
                self.apply_mc_edits(token, raw, EditGroup::Isolated)
            }
            Command::KillWord
            | Command::BackwardKillWord
            | Command::KillLine
            | Command::KillRegion
            | Command::KillRingSave => self.execute_mc_kill(
                token,
                command,
                primary_append_kill,
                &fake_append_kills,
                context.clipboard,
            ),
            Command::Yank => self.execute_mc_yank(token, context.clipboard),
            Command::Undo | Command::UndoRedo => {
                let outcome =
                    self.execute_single_command(token, command, context, primary_append_kill)?;
                self.deactivate_all_marks();
                self.dedup_cursors();
                Ok(outcome)
            }
            _ => Err(RouteError::Unavailable(command)),
        }
    }

    fn execute_mc_delete(
        &mut self,
        token: SessionToken,
        command: Command,
    ) -> Result<CommandOutcome, RouteError> {
        self.buffer.check_editable()?;
        let text = self.buffer.text();
        let delete_region = matches!(
            command,
            Command::DeleteForwardChar | Command::DeleteBackwardChar
        );
        let mut any_region = false;
        let mut raw = Vec::new();
        for id in self.cursor_ids() {
            if delete_region && let Some(region) = self.cursor_region(id) {
                any_region = true;
                raw.push((id, region, String::new()));
                continue;
            }
            let pt = self.cursor_point(id);
            let range = match command {
                Command::DeleteBackwardChar => prev_char(&text, pt).map(|start| start..pt),
                Command::DeleteChar | Command::DeleteForwardChar => {
                    next_char(&text, pt).map(|end| pt..end)
                }
                _ => None,
            };
            if let Some(range) = range {
                raw.push((id, range, String::new()));
            }
        }
        if raw.is_empty() {
            return Err(if command == Command::DeleteBackwardChar {
                RouteError::BeginningOfBuffer
            } else {
                RouteError::EndOfBuffer
            });
        }
        let group = if any_region {
            EditGroup::Isolated
        } else if command == Command::DeleteBackwardChar {
            EditGroup::BackwardDelete
        } else {
            EditGroup::ForwardDelete
        };
        self.apply_mc_edits(token, raw, group)
    }

    fn execute_mc_kill(
        &mut self,
        token: SessionToken,
        command: Command,
        primary_append: bool,
        fake_appends: &[bool],
        clipboard: Option<&dyn SystemClipboard>,
    ) -> Result<CommandOutcome, RouteError> {
        let copy = command == Command::KillRingSave;
        if !copy {
            self.buffer.check_editable()?;
        }
        let text = self.buffer.text();
        let limit = ByteOffset(text.len());
        let mut raw = Vec::new();
        let mut records: Vec<(CursorId, KillRing, Rope, bool, bool)> = Vec::new();
        let mut any_valid = false;

        for id in self.cursor_ids() {
            let pt = self.cursor_point(id);
            let append = match id {
                CursorId::Primary => primary_append,
                CursorId::Fake(i) => fake_appends[i],
            };
            let computed = match command {
                Command::KillWord | Command::BackwardKillWord => {
                    let forward = command == Command::KillWord;
                    let end = motion::word(&text, pt, forward);
                    Some((pt.min(end)..pt.max(end), !forward))
                }
                Command::KillLine => kill::line_range(&text, pt, limit).ok().map(|r| (r, false)),
                _ => self
                    .cursor_mark(id)
                    .map(|mark| (pt.min(mark)..pt.max(mark), pt < mark)),
            };
            let Some((range, prepend)) = computed else {
                if id == CursorId::Primary
                    && matches!(command, Command::KillRegion | Command::KillRingSave)
                {
                    return Err(if copy {
                        RouteError::MarkNotSet
                    } else {
                        RouteError::NoRegion
                    });
                }
                continue;
            };
            any_valid = true;
            let sliced = text.slice(range.start.0..range.end.0);
            records.push((
                id,
                self.cursor_kill_ring(id).clone(),
                sliced,
                append,
                prepend,
            ));
            if !copy && !range.is_empty() {
                raw.push((id, range, String::new()));
            }
        }
        if !any_valid {
            return Err(RouteError::EndOfBuffer);
        }
        if copy {
            for (id, ring, sliced, append, prepend) in records {
                let clip = (id == CursorId::Primary).then_some(clipboard).flatten();
                ring.record(sliced, append, prepend, false, clip);
            }
            self.deactivate_all_marks();
            return Ok(CommandOutcome::default());
        }
        let outcome = if raw.is_empty() {
            self.buffer.break_history_group();
            CommandOutcome {
                reveal_point: true,
                ..CommandOutcome::default()
            }
        } else {
            self.apply_mc_edits(token, raw, EditGroup::Isolated)?
        };
        for (id, ring, sliced, append, prepend) in records {
            let clip = (id == CursorId::Primary).then_some(clipboard).flatten();
            ring.record(sliced, append, prepend, true, clip);
        }
        self.deactivate_all_marks();
        Ok(outcome)
    }

    fn execute_mc_yank(
        &mut self,
        token: SessionToken,
        clipboard: Option<&dyn SystemClipboard>,
    ) -> Result<CommandOutcome, RouteError> {
        self.buffer.check_editable()?;
        let mut raw = Vec::new();
        let mut any_entry = false;
        for id in self.cursor_ids() {
            let pt = self.cursor_point(id);
            let active = match id {
                CursorId::Primary => self.buffer.mark_active(),
                CursorId::Fake(i) => self.fake_cursors[i].mark_active,
            };
            self.set_cursor_mark(id, pt, active)?;
            let clip = (id == CursorId::Primary).then_some(clipboard).flatten();
            if let Some(entry) = self.cursor_kill_ring(id).current_kill(clip) {
                any_entry = true;
                raw.push((id, pt..pt, entry.to_string()));
            }
        }
        if !any_entry {
            return Err(RouteError::EmptyKillRing);
        }
        let mut outcome = self.apply_mc_edits(token, raw, EditGroup::Isolated)?;
        outcome.message = Some("Mark set".to_owned());
        Ok(outcome)
    }

    fn cursor_kill_ring(&self, id: CursorId) -> &KillRing {
        match id {
            CursorId::Primary => &self.kill_ring,
            CursorId::Fake(i) => &self.fake_cursors[i].kill_ring,
        }
    }

    fn apply_mc_edits(
        &mut self,
        token: SessionToken,
        raw: Vec<(CursorId, Range<ByteOffset>, String)>,
        group: EditGroup,
    ) -> Result<CommandOutcome, RouteError> {
        let planned = merge_overlapping_edits(raw);
        let primary_index = planned
            .iter()
            .position(|edit| edit.cursors.contains(&CursorId::Primary));
        let batch: Vec<BatchEdit<'_>> = planned
            .iter()
            .map(|edit| BatchEdit {
                range: edit.range.clone(),
                text: edit.text.as_str(),
            })
            .collect();
        let (update, new_ranges) = self.input.command_edit_batch_grouped(
            &mut self.buffer,
            token,
            &batch,
            group,
            primary_index,
        )?;
        for (edit, new_range) in planned.into_iter().zip(new_ranges) {
            for id in edit.cursors {
                self.set_cursor_point(id, new_range.end)?;
            }
        }
        self.deactivate_all_marks();
        self.dedup_cursors();
        Ok(CommandOutcome {
            update: Some(update),
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    pub(super) fn replace_text_multi(
        &mut self,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
    ) -> Result<InputUpdate, RouteError> {
        let extra: Vec<ByteOffset> = self.fake_cursors.iter().map(FakeCursor::point).collect();
        let batch = self
            .input
            .replace_text_batch(&mut self.buffer, token, range, text, &extra)?;
        for (id, new_range) in batch.ranges {
            if let Some(index) = id {
                self.buffer
                    .set_marker(&self.fake_cursors[index].point, new_range.end)?;
            }
        }
        self.deactivate_all_marks();
        self.dedup_cursors();
        self.interrupt_command_chain();
        Ok(batch.update)
    }

    pub(super) fn commit_preedit_multi(&mut self) -> Result<Option<InputUpdate>, RouteError> {
        let extra: Vec<ByteOffset> = self.fake_cursors.iter().map(FakeCursor::point).collect();
        let Some(batch) = self.input.commit_preedit_batch(&mut self.buffer, &extra)? else {
            return Ok(None);
        };
        for (id, new_range) in batch.ranges {
            if let Some(index) = id {
                self.buffer
                    .set_marker(&self.fake_cursors[index].point, new_range.end)?;
            }
        }
        self.deactivate_all_marks();
        self.dedup_cursors();
        self.interrupt_command_chain();
        Ok(Some(batch.update))
    }
}

#[cfg(test)]
#[path = "cursors_tests.rs"]
mod tests;
