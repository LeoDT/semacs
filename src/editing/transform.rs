use std::ops::Range;

#[cfg(test)]
mod tests;

use super::{CommandOutcome, EditTarget, RouteError};
use crate::buffer::RegionId;
use crate::input::SessionToken;
use crate::text::ByteOffset;

pub(crate) struct RegionTransform {
    region: RegionId,
    token: SessionToken,
    range: Range<ByteOffset>,
}

impl EditTarget {
    pub(crate) fn capture_region(
        &self,
        max_bytes: usize,
    ) -> Result<(RegionTransform, String), String> {
        self.input
            .guard_command(&self.buffer, self.token())
            .map_err(|error| RouteError::from(error).to_string())?;
        if self.is_pending() {
            return Err(RouteError::PrefixPending.to_string());
        }
        let range = self
            .buffer
            .region()
            .ok_or_else(|| RouteError::NoRegion.to_string())?;
        self.buffer
            .check_writable_range(range.clone())
            .map_err(|error| RouteError::from(error).to_string())?;
        if range.end.0 - range.start.0 > max_bytes {
            return Err("Region exceeds the 256 KiB script input limit".into());
        }
        let text = self
            .buffer
            .text()
            .chunks_in_range(range.start.0..range.end.0)
            .collect();
        Ok((
            RegionTransform {
                region: self.buffer.region_id(),
                token: self.token(),
                range,
            },
            text,
        ))
    }

    pub(crate) fn apply_region_transform(
        &mut self,
        target: RegionTransform,
        output: &str,
    ) -> Result<CommandOutcome, String> {
        if self.buffer.region_id() != target.region || self.token() != target.token {
            return Err("Script result discarded: target, text or region changed".into());
        }
        self.input
            .guard_command(&self.buffer, target.token)
            .map_err(|error| RouteError::from(error).to_string())?;
        if self.is_pending() {
            return Err(RouteError::PrefixPending.to_string());
        }
        self.interrupt_command_chain();
        self.buffer.break_history_group();
        self.replace_command(target.token, target.range, output)
            .map_err(|error| error.to_string())
    }
}
