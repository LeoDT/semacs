//! Revert and visible-only auto-revert domain state and helpers.

use std::ops::Range;

use gpui_kit::Task;

use crate::files::FileStamp;
use crate::text::{ByteOffset, Rope};

pub const DEFAULT_INTERVAL_MS: u64 = 1000;
pub const MIN_INTERVAL_MS: u64 = 250;
pub const MAX_INTERVAL_MS: u64 = 60_000;
pub const CHANGE_DEBOUNCE_MS: u64 = 250;
pub const FOCUS_DEBOUNCE_MS: u64 = 150;

/// Application-wide auto-revert configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoRevertSettings {
    pub enabled: bool,
    pub interval_ms: u64,
    pub verbose: bool,
}

impl Default for AutoRevertSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_ms: DEFAULT_INTERVAL_MS,
            verbose: true,
        }
    }
}

impl AutoRevertSettings {
    /// Validates a polling interval in milliseconds against the supported range.
    pub fn validate_interval_ms(value: u64) -> Result<u64, String> {
        if (MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&value) {
            Ok(value)
        } else {
            Err(format!(
                "Auto-revert interval must be between {MIN_INTERVAL_MS} and {MAX_INTERVAL_MS} ms"
            ))
        }
    }
}

/// Per-buffer auto-revert mode override.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutoRevertLocal {
    #[default]
    Inherit,
    Enabled,
    Disabled,
}

impl AutoRevertLocal {
    /// Returns whether auto-revert is active for this buffer given the global flag.
    pub fn is_enabled(self, global: bool) -> bool {
        match self {
            Self::Inherit => global,
            Self::Enabled => true,
            Self::Disabled => false,
        }
    }

    /// Matches Emacs `auto-revert-mode-text` (`" ARev"`) vs empty `global-auto-revert-mode-text`.
    pub fn shows_lighter(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// Per-buffer revert/auto-revert runtime state owned by `BufferSlot`.
#[derive(Default)]
pub(crate) struct RevertState {
    pub local: AutoRevertLocal,
    pub generation: u64,
    pub task: Option<Task<()>>,
    pub deferred: bool,
    pub failed_stamp: Option<FileStamp>,
}

impl RevertState {
    /// Advances and returns the latest-wins generation token for this buffer.
    pub fn next_generation(&mut self) -> u64 {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("revert generation exhausted");
        self.generation
    }
}

/// Outcome of applying a loaded file to an existing buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevertOutcome {
    Unchanged,
    Reverted,
}

/// Computes the minimal UTF-8-safe `(old_range, new_slice)` replacement between
/// `old` and `new`, returning `None` when their contents are byte-identical.
pub fn diff_range<'a>(old: &Rope, new: &'a str) -> Option<(Range<ByteOffset>, &'a str)> {
    let old_len = old.len();
    let new_bytes = new.as_bytes();
    let new_len = new_bytes.len();

    let mut raw_prefix = 0usize;
    'prefix: for chunk in old.chunks() {
        let chunk_bytes = chunk.as_bytes();
        let remaining = new_len - raw_prefix;
        let len = chunk_bytes.len().min(remaining);
        for i in 0..len {
            if chunk_bytes[i] != new_bytes[raw_prefix + i] {
                raw_prefix += i;
                break 'prefix;
            }
        }
        raw_prefix += len;
        if chunk_bytes.len() > remaining {
            break;
        }
    }

    if raw_prefix == old_len && old_len == new_len {
        return None;
    }

    // `old[..raw_prefix] == new[..raw_prefix]`, so flooring on `new` yields a
    // char boundary valid in both `old` and `new`.
    let prefix = new.floor_char_boundary(raw_prefix);
    let max_suffix = (old_len - prefix).min(new_len - prefix);
    if max_suffix == 0 {
        return Some((ByteOffset(prefix)..ByteOffset(old_len), &new[prefix..]));
    }

    let mut raw_suffix = 0usize;
    'suffix: for chunk in old.reversed_chunks_in_range((old_len - max_suffix)..old_len) {
        let chunk_bytes = chunk.as_bytes();
        let remaining = max_suffix - raw_suffix;
        let len = chunk_bytes.len().min(remaining);
        for offset in 0..len {
            let old_byte = chunk_bytes[chunk_bytes.len() - 1 - offset];
            let new_byte = new_bytes[new_len - 1 - raw_suffix - offset];
            if old_byte != new_byte {
                raw_suffix += offset;
                break 'suffix;
            }
        }
        raw_suffix += len;
        if raw_suffix == max_suffix {
            break;
        }
    }

    // Trailing bytes in `old` and `new` match, and UTF-8 char boundaries are
    // uniquely identified by leading (non-continuation) bytes.
    let new_suffix_start = new.ceil_char_boundary(new_len - raw_suffix);
    let suffix = new_len - new_suffix_start;

    Some((
        ByteOffset(prefix)..ByteOffset(old_len - suffix),
        &new[prefix..new_len - suffix],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_range_handles_empty_identical_and_middle_edits() {
        assert_eq!(diff_range(&Rope::from("abc"), "abc"), None);
        assert_eq!(diff_range(&Rope::from(""), ""), None);

        assert_eq!(
            diff_range(&Rope::from(""), "abc"),
            Some((ByteOffset(0)..ByteOffset(0), "abc"))
        );
        assert_eq!(
            diff_range(&Rope::from("abc"), ""),
            Some((ByteOffset(0)..ByteOffset(3), ""))
        );
        assert_eq!(
            diff_range(&Rope::from("hello world"), "hello rust world"),
            Some((ByteOffset(6)..ByteOffset(6), "rust "))
        );
        assert_eq!(
            diff_range(&Rope::from("hello rust world"), "hello world"),
            Some((ByteOffset(6)..ByteOffset(11), ""))
        );
        assert_eq!(
            diff_range(&Rope::from("prefix_old_suffix"), "prefix_new_suffix"),
            Some((ByteOffset(7)..ByteOffset(10), "new"))
        );
        assert_eq!(
            diff_range(&Rope::from("ab"), "aab"),
            Some((ByteOffset(1)..ByteOffset(1), "a"))
        );
        assert_eq!(
            diff_range(&Rope::from("a"), "aa"),
            Some((ByteOffset(1)..ByteOffset(1), "a"))
        );
    }

    #[test]
    fn diff_range_snaps_to_utf8_boundaries_when_bytes_share_prefix_or_suffix() {
        // U+0431 = [0xD0, 0xB1], U+03B1 = [0xCE, 0xB1] (shared trailing 0xB1).
        let old = Rope::from("\u{0431}");
        let new = "\u{03B1}";
        assert_eq!(
            diff_range(&old, new),
            Some((ByteOffset(0)..ByteOffset(2), "\u{03B1}"))
        );

        // Shared leading byte in 4-byte emoji: 😀 (F0 9F 98 80) vs 😁 (F0 9F 98 81).
        let old = Rope::from("a😀b");
        let new = "a😁b";
        assert_eq!(
            diff_range(&old, new),
            Some((ByteOffset(1)..ByteOffset(5), "😁"))
        );

        // CJK prefix/suffix with shared interior change.
        let old = Rope::from("中文旧内容结尾");
        let new = "中文新内容结尾";
        assert_eq!(
            diff_range(&old, new),
            Some((ByteOffset(6)..ByteOffset(9), "新"))
        );
    }

    #[test]
    fn settings_and_local_override_match_contract() {
        let defaults = AutoRevertSettings::default();
        assert!(defaults.enabled);
        assert_eq!(defaults.interval_ms, DEFAULT_INTERVAL_MS);
        assert!(defaults.verbose);

        assert!(AutoRevertSettings::validate_interval_ms(MIN_INTERVAL_MS).is_ok());
        assert!(AutoRevertSettings::validate_interval_ms(MAX_INTERVAL_MS).is_ok());
        assert!(AutoRevertSettings::validate_interval_ms(MIN_INTERVAL_MS - 1).is_err());
        assert!(AutoRevertSettings::validate_interval_ms(MAX_INTERVAL_MS + 1).is_err());

        assert!(AutoRevertLocal::Inherit.is_enabled(true));
        assert!(!AutoRevertLocal::Inherit.is_enabled(false));
        assert!(AutoRevertLocal::Enabled.is_enabled(false));
        assert!(!AutoRevertLocal::Disabled.is_enabled(true));

        assert!(!AutoRevertLocal::Inherit.shows_lighter());
        assert!(AutoRevertLocal::Enabled.shows_lighter());
        assert!(!AutoRevertLocal::Disabled.shows_lighter());
    }
}
