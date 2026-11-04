use std::ops::Range;

use crate::buffer::EditError;
use crate::command::Command;
use crate::input::SessionToken;
use crate::text::{ByteOffset, Point, Rope};

use super::{CommandOutcome, EditTarget, RouteError, motion};

fn line_range(
    text: &Rope,
    point: ByteOffset,
    limit: ByteOffset,
) -> Result<Range<ByteOffset>, RouteError> {
    if point.0 == text.len() {
        return Err(RouteError::EndOfBuffer);
    }
    let row = text.offset_to_point(point.0).row;
    let end = text
        .point_to_offset(Point::new(row, text.line_len(row)))
        .min(limit.0);
    let blanks = text
        .chunks_in_range(point.0..end)
        .flat_map(str::chars)
        .all(|ch| matches!(ch, ' ' | '\t'));
    let end = if blanks && end < limit.0 {
        end + 1
    } else {
        end
    };
    Ok(point..ByteOffset(end))
}

impl EditTarget {
    pub(super) fn kill(
        &mut self,
        token: SessionToken,
        command: Command,
        append: bool,
    ) -> Result<CommandOutcome, RouteError> {
        let point = self.buffer.point();
        let copy = command == Command::KillRingSave;
        let (range, prepend) = match command {
            Command::KillWord | Command::BackwardKillWord => {
                let forward = command == Command::KillWord;
                let end = motion::word(&self.buffer.text(), point, forward);
                (point.min(end)..point.max(end), !forward)
            }
            Command::KillLine => (
                line_range(&self.buffer.text(), point, self.field_range().end)?,
                false,
            ),
            _ => {
                let mark = self.buffer.mark().ok_or(if copy {
                    RouteError::MarkNotSet
                } else {
                    RouteError::NoRegion
                })?;
                (point.min(mark)..point.max(mark), point < mark)
            }
        };
        let text = self.buffer.text().slice(range.start.0..range.end.0);
        let read_only = (!range.is_empty())
            .then(|| self.buffer.check_writable_range(range.clone()).err())
            .flatten();
        if copy || read_only.is_some() {
            self.kill_ring.record(text, append, prepend, !copy);
            self.buffer.deactivate_mark();
            if let Some(error) = read_only.filter(|_| !copy) {
                match command {
                    Command::KillLine | Command::KillWord => self.buffer.set_point(range.end)?,
                    Command::BackwardKillWord => self.buffer.set_point(range.start)?,
                    _ => {}
                }
                return Err(error.into());
            }
            return Ok(CommandOutcome::default());
        }
        let outcome = self.replace_command(token, range, "")?;
        self.kill_ring.record(text, append, prepend, true);
        self.buffer.deactivate_mark();
        Ok(outcome)
    }

    pub(super) fn yank(&mut self, token: SessionToken) -> Result<CommandOutcome, RouteError> {
        if self.buffer.is_read_only() {
            return Err(EditError::ReadOnly.into());
        }
        let point = self.buffer.point();
        let active = self.buffer.mark_active();
        self.buffer.set_mark(point)?;
        if !active {
            self.buffer.deactivate_mark();
        }
        let text = self.kill_ring.latest().ok_or(RouteError::EmptyKillRing)?;
        let mut outcome = self.replace_command(token, point..point, &text.to_string())?;
        outcome.message = Some("Mark set".to_owned());
        Ok(outcome)
    }
}
