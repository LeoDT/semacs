//! Shared editing target and command execution, independent of window furniture.

use std::fmt;
use std::ops::Range;

use gpui_kit::{KeyDownEvent, Keystroke};

use crate::buffer::{Buffer, EditError, EditGroup, SavePoint};
use crate::command::{Command, Scope};
use crate::command_registry::CommandId;
use crate::input::{
    DisplayChange, DisplaySnapshot, InputError, InputSession, InputUpdate, SessionToken,
};
use crate::keymap::{Dispatch, Keymap, KeymapState};
use crate::text::{ByteOffset, OffsetUtf16, Point, Rope};

mod column;
mod field;
mod indent;
mod kill;
mod kill_ring;
mod motion;
mod transform;
mod viewport;

pub(crate) use transform::RegionTransform;

pub use kill_ring::KillRing;
pub use viewport::{CommandViewport, PageMotion, RecenterPosition, RowMotion, ViewportRequest};

pub(crate) const TAB_WIDTH: u32 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    Input(InputError),
    PrefixPending,
    BeginningOfBuffer,
    EndOfBuffer,
    LayoutNotReady,
    MarkNotSet,
    NoRegion,
    EmptyKillRing,
    NoUndoInformation,
    NoRedoInformation,
    Unavailable(Command),
}

impl From<InputError> for RouteError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

impl From<EditError> for RouteError {
    fn from(error: EditError) -> Self {
        Self::Input(InputError::Edit(error))
    }
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(InputError::CompositionActive) => {
                f.write_str("Input method composition is active; finish it first")
            }
            Self::Input(InputError::Edit(EditError::ReadOnly)) => {
                f.write_str("Buffer is read-only")
            }
            Self::Input(InputError::Edit(EditError::EditingInhibited(reason))) => {
                f.write_str(reason)
            }
            Self::Input(InputError::Edit(EditError::TextReadOnly)) => {
                f.write_str("Text is read-only")
            }
            Self::PrefixPending => f.write_str("Key sequence is pending"),
            Self::BeginningOfBuffer => f.write_str("Beginning of buffer"),
            Self::EndOfBuffer => f.write_str("End of buffer"),
            Self::LayoutNotReady => f.write_str("Text layout is not ready; try again"),
            Self::MarkNotSet => f.write_str("The mark is not set now"),
            Self::NoRegion => f.write_str("The mark is not set now, so there is no region"),
            Self::EmptyKillRing => f.write_str("Kill ring is empty"),
            Self::NoUndoInformation => f.write_str("No further undo information"),
            Self::NoRedoInformation => f.write_str("No further redo information"),
            Self::Unavailable(command) => write!(f, "{} is not available yet", command.name()),
            Self::Input(error) => write!(f, "Input rejected: {error:?}"),
        }
    }
}

impl std::error::Error for RouteError {}

#[derive(Debug, Default)]
pub struct CommandOutcome {
    pub update: Option<InputUpdate>,
    pub message: Option<String>,
    pub reveal_point: bool,
    pub viewport: Option<ViewportRequest>,
    pub quit: bool,
}

impl CommandOutcome {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDisposition {
    TextInput,
    Handled,
    /// The command belongs to a layer above the edit target. The owner must
    /// execute it; the target has not changed any state for it.
    Requested(CommandId),
    Ignored,
}

#[derive(Debug)]
pub struct Route {
    pub disposition: InputDisposition,
    pub outcome: CommandOutcome,
}

impl Route {
    fn handled(outcome: CommandOutcome) -> Self {
        Self {
            disposition: InputDisposition::Handled,
            outcome,
        }
    }

    fn requested(command: impl Into<CommandId>) -> Self {
        Self {
            disposition: InputDisposition::Requested(command.into()),
            outcome: CommandOutcome::default(),
        }
    }

    fn input(disposition: InputDisposition) -> Self {
        Self {
            disposition,
            outcome: CommandOutcome::default(),
        }
    }
}

pub struct EditTarget {
    buffer: Buffer,
    input: InputSession,
    keys: KeymapState,
    kill_ring: KillRing,
    last_command: Option<Command>,
    goal_column: Option<usize>,
    recenter_position: RecenterPosition,
    field: Option<field::FieldBoundary>,
}

impl EditTarget {
    pub fn new(buffer: Buffer) -> Self {
        Self::with_kill_ring(buffer, KillRing::default())
    }

    pub fn with_kill_ring(buffer: Buffer, kill_ring: KillRing) -> Self {
        Self {
            input: InputSession::new(&buffer),
            buffer,
            keys: KeymapState::default(),
            kill_ring,
            last_command: None,
            goal_column: None,
            recenter_position: RecenterPosition::Center,
            field: None,
        }
    }

    pub fn with_field_boundary(mut self, boundary: ByteOffset) -> Result<Self, EditError> {
        if boundary > self.buffer.read_only_prefix()
            || !self.buffer.text().is_char_boundary(boundary.0)
        {
            return Err(EditError::InvalidRange);
        }
        self.field = Some(field::FieldBoundary(boundary));
        Ok(self)
    }

    fn field_range(&self) -> Range<ByteOffset> {
        self.field.map_or(
            ByteOffset(0)..ByteOffset(self.buffer.text().len()),
            |field| field.range(self.buffer.point(), self.buffer.text().len()),
        )
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn kill_ring(&self) -> &KillRing {
        &self.kill_ring
    }

    pub fn input(&self) -> &InputSession {
        &self.input
    }

    pub(crate) fn synchronize_buffer(&mut self, old_len: usize) -> Option<DisplayChange> {
        let change = self.input.synchronize_buffer(&self.buffer, old_len)?;
        self.keys.clear();
        self.last_command = None;
        self.goal_column = None;
        Some(change)
    }

    pub fn token(&self) -> SessionToken {
        self.input.token()
    }

    pub fn is_pending(&self) -> bool {
        self.keys.is_pending()
    }

    pub(crate) fn clear_pending(&mut self) {
        self.keys.clear();
        self.buffer.break_history_group();
        self.interrupt_command_chain();
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.buffer.break_history_group();
        self.buffer.set_read_only(read_only);
    }

    pub fn mark_saved_at(&mut self, point: SavePoint) -> Result<(), EditError> {
        self.buffer.mark_saved_at(point)
    }

    pub fn begin_save(&mut self) -> SavePoint {
        self.buffer.begin_save()
    }

    pub fn display_snapshot(&self) -> DisplaySnapshot {
        self.input
            .display_snapshot(&self.buffer)
            .unwrap_or_else(|_| self.input.committed_snapshot(&self.buffer))
    }

    pub fn snapshot_for(&self, token: SessionToken) -> Result<DisplaySnapshot, InputError> {
        self.input.snapshot_for(&self.buffer, token)
    }

    pub fn allows_text_input(&self, token: SessionToken) -> bool {
        let point = self.buffer.point();
        !self.is_pending()
            && self.buffer.check_writable_range(point..point).is_ok()
            && self.snapshot_for(token).is_ok()
    }

    pub fn end_input(&mut self) -> DisplayChange {
        self.buffer.break_history_group();
        let before = self.display_snapshot();
        self.input.end();
        self.keys.clear();
        self.interrupt_command_chain();
        let range = before
            .marked_range()
            .unwrap_or(self.buffer.point()..self.buffer.point());
        DisplayChange {
            before: before.id(),
            after: self.display_snapshot().id(),
            new_range: range.start..range.start,
            old_range: range,
        }
    }

    pub fn resume_input(&mut self) -> Option<DisplayChange> {
        if self.input.display_snapshot(&self.buffer).is_ok() {
            return None;
        }
        let before = self.display_snapshot().id();
        self.buffer.break_history_group();
        self.input = InputSession::new(&self.buffer);
        self.keys.clear();
        self.interrupt_command_chain();
        let point = self.buffer.point();
        Some(DisplayChange {
            before,
            after: self.display_snapshot().id(),
            old_range: point..point,
            new_range: point..point,
        })
    }

    pub fn key_down(
        &mut self,
        token: SessionToken,
        event: &KeyDownEvent,
        layers: &[&Keymap],
    ) -> Result<Route, RouteError> {
        self.key_down_with_viewport(token, event, layers, None)
    }

    pub fn key_down_with_viewport(
        &mut self,
        token: SessionToken,
        event: &KeyDownEvent,
        layers: &[&Keymap],
        viewport: Option<&dyn CommandViewport>,
    ) -> Result<Route, RouteError> {
        self.dispatch_keystroke(
            token,
            &event.keystroke,
            event.prefer_character_input,
            layers,
            viewport,
        )
    }

    pub fn dispatch_keystroke(
        &mut self,
        token: SessionToken,
        stroke: &Keystroke,
        prefer_character_input: bool,
        layers: &[&Keymap],
        viewport: Option<&dyn CommandViewport>,
    ) -> Result<Route, RouteError> {
        self.snapshot_for(token)?;
        let composing = self.input.is_composing();
        let dispatch = if composing {
            KeymapState::default().dispatch_keystroke(stroke, prefer_character_input, layers)
        } else {
            self.keys
                .dispatch_keystroke(stroke, prefer_character_input, layers)
        };
        if composing
            && !matches!(
                dispatch,
                Dispatch::SelfInsert | Dispatch::PlatformInput | Dispatch::Ignored
            )
        {
            return Err(InputError::CompositionActive.into());
        }
        match dispatch {
            Dispatch::Command(command) if !command.available_in(self.buffer.mode()) => {
                Err(RouteError::Unavailable(command))
            }
            Dispatch::Command(command) => match command.scope() {
                Scope::Edit => self
                    .execute_with_viewport(token, command, viewport)
                    .map(Route::handled),
                Scope::Application | Scope::ReadSession => {
                    self.buffer.break_history_group();
                    self.interrupt_command_chain();
                    Ok(Route::requested(command))
                }
            },
            Dispatch::Script(command) => {
                self.buffer.break_history_group();
                self.interrupt_command_chain();
                Ok(Route::requested(CommandId::Script(command)))
            }
            Dispatch::Pending(message) => {
                self.last_command = None;
                Ok(Route::handled(CommandOutcome::message(message)))
            }
            Dispatch::Undefined(message) => {
                self.buffer.break_history_group();
                self.interrupt_command_chain();
                Ok(Route::handled(CommandOutcome::message(message)))
            }
            Dispatch::SelfInsert | Dispatch::PlatformInput => {
                self.guard_input(token)?;
                let point = self.buffer.point();
                if let Err(error) = self.buffer.check_writable_range(point..point) {
                    self.interrupt_command_chain();
                    return Err(error.into());
                }
                Ok(Route::input(InputDisposition::TextInput))
            }
            Dispatch::InputBlocked => Err(RouteError::PrefixPending),
            Dispatch::Ignored => Ok(Route::input(InputDisposition::Ignored)),
        }
    }

    pub fn execute(
        &mut self,
        token: SessionToken,
        command: Command,
    ) -> Result<CommandOutcome, RouteError> {
        self.execute_with_viewport(token, command, None)
    }

    pub fn execute_with_viewport(
        &mut self,
        token: SessionToken,
        command: Command,
        viewport: Option<&dyn CommandViewport>,
    ) -> Result<CommandOutcome, RouteError> {
        if command.scope() != Scope::Edit || !command.available_in(self.buffer.mode()) {
            return Err(RouteError::Unavailable(command));
        }
        self.input.guard_command(&self.buffer, token)?;
        if self.is_pending() {
            return Err(RouteError::PrefixPending);
        }
        if !matches!(
            self.last_command,
            Some(Command::NextLine | Command::PreviousLine)
        ) {
            self.goal_column = None;
        }
        if self.last_command != Some(Command::Recenter) {
            self.recenter_position = RecenterPosition::Center;
        }
        if !matches!(
            command,
            Command::DeleteChar | Command::DeleteForwardChar | Command::DeleteBackwardChar
        ) {
            self.buffer.break_history_group();
        }
        let append_kill = self.kill_ring.begin_command();
        let result = self.execute_command(token, command, viewport, append_kill);
        if result.is_err() {
            self.buffer.break_history_group();
        }
        self.last_command = Some(command);
        result
    }

    fn execute_command(
        &mut self,
        token: SessionToken,
        command: Command,
        viewport: Option<&dyn CommandViewport>,
        append_kill: bool,
    ) -> Result<CommandOutcome, RouteError> {
        let point = self.buffer.point();
        let field = self.field_range();
        let text = &self.buffer.text();
        let next = || {
            text.chars_at(point.0)
                .next()
                .map(|ch| ByteOffset(point.0 + ch.len_utf8()))
        };
        let previous = || {
            text.reversed_chars_at(point.0)
                .next()
                .map(|ch| ByteOffset(point.0 - ch.len_utf8()))
        };
        match command {
            Command::ForwardChar => {
                self.buffer
                    .set_point(next().ok_or(RouteError::EndOfBuffer)?)?;
            }
            Command::BackwardChar => {
                self.buffer
                    .set_point(previous().ok_or(RouteError::BeginningOfBuffer)?)?;
            }
            Command::MoveBeginningOfLine => {
                let row = text.offset_to_point(point.0).row;
                self.buffer.set_point(
                    ByteOffset(text.point_to_offset(Point::new(row, 0))).max(field.start),
                )?;
            }
            Command::MoveEndOfLine => {
                let row = text.offset_to_point(point.0).row;
                self.buffer.set_point(ByteOffset(
                    text.point_to_offset(Point::new(row, text.line_len(row))),
                ))?;
            }
            Command::ForwardWord | Command::BackwardWord => {
                self.buffer.set_point(motion::word(
                    text,
                    point,
                    command == Command::ForwardWord,
                ))?;
            }
            Command::ForwardParagraph | Command::BackwardParagraph => {
                self.buffer.set_point(motion::paragraph(
                    text,
                    point,
                    command == Command::ForwardParagraph,
                ))?;
            }
            Command::NextLine | Command::PreviousLine => {
                let forward = command == Command::NextLine;
                let Some(destination) =
                    self.vertical_destination(text, point, forward, viewport)?
                else {
                    self.buffer
                        .set_point(ByteOffset(if forward { text.len() } else { 0 }))?;
                    return Err(if forward {
                        RouteError::EndOfBuffer
                    } else {
                        RouteError::BeginningOfBuffer
                    });
                };
                self.buffer.set_point(destination)?;
            }
            Command::BeginningOfBuffer | Command::EndOfBuffer => {
                let destination = if command == Command::BeginningOfBuffer {
                    ByteOffset(0)
                } else {
                    ByteOffset(text.len())
                };
                let set_mark = !self.buffer.mark_active();
                if set_mark {
                    self.buffer.set_mark(point)?;
                    self.buffer.deactivate_mark();
                }
                self.buffer.set_point(destination)?;
                return Ok(CommandOutcome {
                    reveal_point: true,
                    // `end-of-buffer` recenters three lines above the bottom
                    // when the end is off screen; `beginning-of-buffer` leaves
                    // it to redisplay.
                    viewport: (command == Command::EndOfBuffer)
                        .then_some(ViewportRequest::RecenterFromBottom(3)),
                    message: set_mark.then(|| "Mark set".to_owned()),
                    ..CommandOutcome::default()
                });
            }
            Command::ScrollUp | Command::ScrollDown => {
                let motion = viewport
                    .ok_or(RouteError::LayoutNotReady)?
                    .page(point, command == Command::ScrollUp)?;
                self.buffer.set_point(motion.point)?;
                return Ok(CommandOutcome {
                    reveal_point: true,
                    viewport: Some(ViewportRequest::ScrollToRow(motion.first_row)),
                    ..CommandOutcome::default()
                });
            }
            Command::Recenter => {
                let position = self.recenter_position;
                self.recenter_position = position.next();
                return Ok(CommandOutcome {
                    viewport: Some(ViewportRequest::Recenter(position)),
                    ..CommandOutcome::default()
                });
            }
            Command::DeleteForwardChar
                if self.buffer.mark_active() && self.buffer.mark() != Some(point) =>
            {
                let mark = self.buffer.mark().expect("active mark");
                return self.replace_command(token, point.min(mark)..point.max(mark), "");
            }
            Command::DeleteChar | Command::DeleteForwardChar => {
                let end = next().ok_or(RouteError::EndOfBuffer)?;
                return self.replace_command_grouped(
                    token,
                    point..end,
                    "",
                    EditGroup::ForwardDelete,
                );
            }
            Command::DeleteBackwardChar => {
                let start = previous().ok_or(RouteError::BeginningOfBuffer)?;
                return self.replace_command_grouped(
                    token,
                    start..point,
                    "",
                    EditGroup::BackwardDelete,
                );
            }
            Command::Newline => {
                let (range, inserted) = indent::newline(text, point, field.start);
                return self.replace_command(token, range, &inserted);
            }
            Command::ElectricNewline => return self.replace_command(token, point..point, "\n"),
            Command::Indent => {
                let (range, inserted) = indent::tab(text, point, field.start);
                return self.replace_command(token, range, &inserted);
            }
            Command::SetMark => {
                if self.last_command == Some(Command::SetMark) && self.buffer.mark_active() {
                    self.buffer.deactivate_mark();
                    return Ok(CommandOutcome::message("Mark deactivated"));
                }
                self.buffer.set_mark(point)?;
                return Ok(CommandOutcome::message("Mark set"));
            }
            Command::KeyboardQuit => {
                self.buffer.deactivate_mark();
                return Ok(CommandOutcome {
                    quit: true,
                    ..CommandOutcome::message("Quit")
                });
            }
            Command::KeyboardEscapeQuit => {
                self.buffer.deactivate_mark();
                return Ok(CommandOutcome {
                    quit: true,
                    ..CommandOutcome::default()
                });
            }
            Command::KillWord
            | Command::BackwardKillWord
            | Command::KillLine
            | Command::KillRegion
            | Command::KillRingSave => {
                return self.kill(token, command, append_kill);
            }
            Command::Yank => return self.yank(token),
            Command::Undo | Command::UndoRedo => {
                let redo = command == Command::UndoRedo;
                let update = self
                    .input
                    .replay_history(&mut self.buffer, token, redo)?
                    .ok_or(if redo {
                        RouteError::NoRedoInformation
                    } else {
                        RouteError::NoUndoInformation
                    })?;
                return Ok(CommandOutcome {
                    update: Some(update),
                    message: Some(if redo { "Redo" } else { "Undo" }.to_owned()),
                    reveal_point: true,
                    ..CommandOutcome::default()
                });
            }
            // Every other command is owned by a layer above the edit target.
            _ => return Err(RouteError::Unavailable(command)),
        }
        Ok(CommandOutcome {
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    /// The destination one line away: a display row when the caller supplied a
    /// viewport over soft-wrapped text, otherwise the sections projection or a
    /// logical row. `None` means there is no line in that direction.
    fn vertical_destination(
        &mut self,
        text: &Rope,
        point: ByteOffset,
        forward: bool,
        viewport: Option<&dyn CommandViewport>,
    ) -> Result<Option<ByteOffset>, RouteError> {
        let sections = self.buffer.sections();
        let (current, target) = match (sections.as_ref(), viewport) {
            (None, Some(viewport)) => {
                let RowMotion { current, target } = viewport.adjacent_row(point, forward)?;
                (current, target)
            }
            _ => {
                let row = text.offset_to_point(point.0).row;
                let destination = match &sections {
                    Some(sections) => sections
                        .adjacent_line(point, forward)
                        .map(|offset| text.offset_to_point(offset.0).row),
                    None if forward => (row < text.max_point().row).then_some(row + 1),
                    None => row.checked_sub(1),
                };
                (
                    Self::line_range(text, row),
                    destination.map(|destination| Self::line_range(text, destination)),
                )
            }
        };
        let goal = *self
            .goal_column
            .get_or_insert_with(|| column::column(text, current.start.0..point.0));
        Ok(target.map(|target| column::offset(text, target.start.0..target.end.0, goal)))
    }

    fn line_range(text: &Rope, row: u32) -> Range<ByteOffset> {
        let start = text.point_to_offset(Point::new(row, 0));
        ByteOffset(start)..ByteOffset(start + text.line_len(row) as usize)
    }

    fn replace_command(
        &mut self,
        token: SessionToken,
        range: Range<ByteOffset>,
        text: &str,
    ) -> Result<CommandOutcome, RouteError> {
        self.replace_command_grouped(token, range, text, EditGroup::Isolated)
    }

    /// Replaces committed text through the shared edit entry as one command
    /// edit. Used by the owner of a read session (completion, kind-specific
    /// rewrites), never by platform input.
    pub fn replace_text_range(
        &mut self,
        token: SessionToken,
        range: Range<ByteOffset>,
        text: &str,
    ) -> Result<InputUpdate, RouteError> {
        let update = self.input.command_edit_grouped(
            &mut self.buffer,
            token,
            range,
            text,
            EditGroup::Isolated,
        )?;
        self.interrupt_command_chain();
        Ok(update)
    }

    fn replace_command_grouped(
        &mut self,
        token: SessionToken,
        range: Range<ByteOffset>,
        text: &str,
        group: EditGroup,
    ) -> Result<CommandOutcome, RouteError> {
        let update =
            self.input
                .command_edit_grouped(&mut self.buffer, token, range, text, group)?;
        Ok(CommandOutcome {
            update: Some(update),
            reveal_point: true,
            ..CommandOutcome::default()
        })
    }

    pub(crate) fn show_point(&mut self, point: ByteOffset) {
        self.buffer.set_point(point).expect("valid display point");
    }

    /// Moves only the point, without a mark or region. Mouse presses and drags
    /// use [`Self::press_pointer`], [`Self::drag_pointer`] and
    /// [`Self::release_pointer`]; this helper stays for callers that just place
    /// the caret at a hit-tested offset.
    pub fn set_point_from_pointer(
        &mut self,
        token: SessionToken,
        point: ByteOffset,
    ) -> Result<(), RouteError> {
        self.input.guard_command(&self.buffer, token)?;
        self.buffer.set_point(point)?;
        self.buffer.break_history_group();
        self.keys.clear();
        self.interrupt_command_chain();
        Ok(())
    }

    /// Starts a mouse selection: mark and point at the pressed offset and the
    /// region is active until the gesture ends. The press is the only part of a
    /// drag that starts a new history group.
    pub fn press_pointer(
        &mut self,
        token: SessionToken,
        point: ByteOffset,
    ) -> Result<(), RouteError> {
        self.input.guard_command(&self.buffer, token)?;
        self.buffer.set_point(point)?;
        let point = self.buffer.point();
        self.buffer.set_mark(point)?;
        self.buffer.break_history_group();
        self.keys.clear();
        self.interrupt_command_chain();
        Ok(())
    }

    /// Extends a mouse selection without starting another history group or
    /// command chain.
    pub fn drag_pointer(
        &mut self,
        token: SessionToken,
        point: ByteOffset,
    ) -> Result<(), RouteError> {
        self.input.guard_command(&self.buffer, token)?;
        self.buffer.set_point(point)?;
        Ok(())
    }

    /// Finishes a mouse selection. A press released without moving the point to
    /// another offset is a plain click and leaves no active region.
    pub fn release_pointer(&mut self, click: bool) {
        if click {
            self.buffer.deactivate_mark();
        }
    }

    fn interrupt_command_chain(&mut self) {
        self.last_command = None;
        self.kill_ring.break_chain();
    }

    fn guard_input(&self, token: SessionToken) -> Result<(), RouteError> {
        self.snapshot_for(token)?;
        if self.is_pending() {
            return Err(RouteError::PrefixPending);
        }
        Ok(())
    }

    pub fn replace_text(
        &mut self,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
    ) -> Result<InputUpdate, RouteError> {
        self.guard_input(token)?;
        let update = self
            .input
            .replace_text(&mut self.buffer, token, range, text)?;
        self.interrupt_command_chain();
        Ok(update)
    }

    pub fn replace_and_mark(
        &mut self,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
        selected: Option<Range<OffsetUtf16>>,
    ) -> Result<InputUpdate, RouteError> {
        self.guard_input(token)?;
        let update = self
            .input
            .replace_and_mark(&mut self.buffer, token, range, text, selected)?;
        if update.edit.is_some() {
            self.interrupt_command_chain();
        }
        Ok(update)
    }

    pub fn commit_preedit(
        &mut self,
        token: SessionToken,
    ) -> Result<Option<InputUpdate>, RouteError> {
        self.guard_input(token)?;
        let update = self.input.commit_preedit(&mut self.buffer)?;
        if update.is_some() {
            self.interrupt_command_chain();
        }
        Ok(update)
    }
}
