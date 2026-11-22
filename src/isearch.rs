//! Incremental search state machine, matcher, history ring, and lazy-highlight scanner.
//!
//! Models GNU Emacs 31.1 `isearch.el` behavior (state stack, smart-case,
//! two-stage `C-g` abort, wrap pause, direction reversal, yanking from buffer
//! and kill ring, search rings, and temporary expansion of hidden sections).
//! Regular expression mode uses Rust's `regex` syntax (consistent with F8 `rg`),
//! with multiline `^`/`$` and non-newline `.` matching Emacs line semantics.

use std::ops::Range;

use gpui_kit::Global;
use regex::{Regex, RegexBuilder};

use crate::text::ByteOffset;

pub const SEARCH_RING_MAX: usize = 16;
const REGEX_SIZE_LIMIT: usize = 10 * 1024 * 1024;
const LAZY_HIGHLIGHT_LIMIT: usize = 512;
const BACKWARD_CHUNK_BYTES: usize = 64 * 1024;

/// Application-wide search history rings (`search-ring` and `regexp-search-ring`).
#[derive(Clone, Debug, Default)]
pub struct IsearchRing {
    literal: Vec<String>,
    regexp: Vec<String>,
}

impl Global for IsearchRing {}

impl IsearchRing {
    pub fn push(&mut self, query: &str, regexp: bool) {
        if query.is_empty() {
            return;
        }
        let ring = if regexp {
            &mut self.regexp
        } else {
            &mut self.literal
        };
        ring.retain(|entry| entry != query);
        ring.insert(0, query.to_owned());
        ring.truncate(SEARCH_RING_MAX);
    }

    pub fn latest(&self, regexp: bool) -> Option<&str> {
        self.get(regexp, 0)
    }

    pub fn get(&self, regexp: bool, index: usize) -> Option<&str> {
        let ring = if regexp { &self.regexp } else { &self.literal };
        ring.get(index).map(String::as_str)
    }

    pub fn len(&self, regexp: bool) -> usize {
        if regexp {
            self.regexp.len()
        } else {
            self.literal.len()
        }
    }

    pub fn is_empty(&self, regexp: bool) -> bool {
        self.len(regexp) == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Literal,
    Regexp,
    Word,
    Symbol,
}

impl SearchMode {
    pub fn uses_regexp_ring(self) -> bool {
        self == Self::Regexp
    }
}

/// Snapshot of one isearch state pushed onto `isearch-cmds` for `DEL` / `C-g` undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsearchState {
    pub query: String,
    pub forward: bool,
    pub mode: SearchMode,
    /// `None` = auto smart-case (`case-fold-search = t`, `search-upper-case = not-yanks`);
    /// `Some(true)` = forced case-insensitive; `Some(false)` = forced case-sensitive.
    pub case_fold_override: Option<bool>,
    pub lax_whitespace: bool,
    /// `true` = `search-invisible = 'open` (match & temporarily expand hidden sections);
    /// `false` = `search-invisible = nil` (skip hidden sections).
    pub search_invisible: bool,
    pub point: ByteOffset,
    pub other_end: Option<ByteOffset>,
    pub barrier: ByteOffset,
    pub success: bool,
    pub wrapped: bool,
    pub error: Option<String>,
    pub just_started: bool,
    /// Set by `M-s .` (`isearch-forward-symbol-at-point`) for strict right-boundary matching.
    pub strict_bounds: bool,
}

impl IsearchState {
    pub fn effective_case_fold(&self) -> bool {
        match self.case_fold_override {
            Some(forced) => forced,
            None => no_upper_case(&self.query, self.mode == SearchMode::Regexp),
        }
    }

    pub fn match_range(&self) -> Option<Range<ByteOffset>> {
        if !self.success || self.query.is_empty() {
            return None;
        }
        let other = self.other_end?;
        Some(self.point.min(other)..self.point.max(other))
    }
}

/// Result of pressing `C-g` (`isearch-abort`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortOutcome {
    /// Failing or invalid regexp popped back to the last successful state; stay in isearch.
    Reverted,
    /// Search was succeeding; cancel completely and return to `opoint`.
    Cancelled,
}

/// Core incremental search engine, independent of GPUI.
#[derive(Clone, Debug)]
pub struct IsearchEngine {
    pub opoint: ByteOffset,
    pub state: IsearchState,
    cmds: Vec<IsearchState>,
    ring_index: Option<usize>,
    yank_prev_point: Option<ByteOffset>,
    pub status_hint: Option<String>,
}

impl IsearchEngine {
    pub fn new(opoint: ByteOffset, forward: bool, mode: SearchMode) -> Self {
        let state = IsearchState {
            query: String::new(),
            forward,
            mode,
            case_fold_override: None,
            lax_whitespace: true,
            search_invisible: true,
            point: opoint,
            other_end: None,
            barrier: opoint,
            success: true,
            wrapped: false,
            error: None,
            just_started: true,
            strict_bounds: false,
        };
        Self {
            opoint,
            cmds: vec![state.clone()],
            state,
            ring_index: None,
            yank_prev_point: None,
            status_hint: None,
        }
    }

    /// Starts `M-s .` (`isearch-forward-symbol-at-point`).
    pub fn start_symbol_at_point(
        opoint: ByteOffset,
        text: &str,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> Self {
        let mut engine = Self::new(opoint, true, SearchMode::Symbol);
        if let Some((start, end)) = symbol_bounds_at(text, opoint.0) {
            engine.state.point = ByteOffset(start);
            engine.state.barrier = ByteOffset(start);
            engine.state.strict_bounds = true;
            engine.cmds[0] = engine.state.clone();
            let raw = text[start..end].to_owned();
            engine.yank_string(text, &raw, is_hidden);
        } else {
            engine.state.error = Some("No symbol at point".into());
            engine.cmds.push(engine.state.clone());
        }
        engine
    }

    /// Clamps all stored offsets to valid UTF-8 character boundaries in `text`
    /// after an out-of-band buffer snapshot change.
    pub fn clamp_to_text(&mut self, text: &str) {
        self.opoint = ByteOffset(floor_char_boundary(text, self.opoint.0));
        self.yank_prev_point = self
            .yank_prev_point
            .map(|offset| ByteOffset(floor_char_boundary(text, offset.0)));
        clamp_state(&mut self.state, text);
        for cmd in &mut self.cmds {
            clamp_state(cmd, text);
        }
    }

    pub fn match_range(&self) -> Option<Range<ByteOffset>> {
        self.state.match_range()
    }

    pub fn has_pushed_states(&self) -> bool {
        self.cmds.len() > 1
    }

    fn push_state(&mut self) {
        self.cmds.push(self.state.clone());
    }

    fn pop_state(&mut self) -> bool {
        if self.cmds.len() <= 1 {
            return false;
        }
        self.cmds.pop();
        if let Some(previous) = self.cmds.last() {
            self.state = previous.clone();
        }
        true
    }

    /// Appends literal user input (typed char, TAB, C-j, IME commit, quoted char).
    pub fn append_text(
        &mut self,
        text: &str,
        addition: &str,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        if addition.is_empty() {
            return;
        }
        self.status_hint = None;
        self.ring_index = None;
        self.state.query.push_str(addition);
        self.search_and_update(text, false, is_hidden);
    }

    /// Appends yanked text (`C-w`, `M-s C-w`, `M-s C-e`, `C-y`), applying Emacs
    /// `search-upper-case = not-yanks` downcasing and regexp escaping.
    pub fn yank_string(
        &mut self,
        text: &str,
        raw: &str,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        if raw.is_empty() {
            return;
        }
        self.status_hint = None;
        self.ring_index = None;
        let mut pulled = if self.state.effective_case_fold() {
            raw.to_lowercase()
        } else {
            raw.to_owned()
        };
        if self.state.mode == SearchMode::Regexp {
            pulled = regex::escape(&pulled);
        }
        self.state.query.push_str(&pulled);
        self.search_and_update(text, true, is_hidden);
    }

    pub fn yank_word_or_char(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        let (pulled, end) = self.pull_from_buffer(text, next_word_or_char_end);
        self.yank_string(text, &pulled, is_hidden);
        self.yank_prev_point = if self.state.success { None } else { Some(end) };
    }

    pub fn yank_symbol_or_char(
        &mut self,
        text: &str,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        let (pulled, end) = self.pull_from_buffer(text, next_symbol_or_char_end);
        self.yank_string(text, &pulled, is_hidden);
        self.yank_prev_point = if self.state.success { None } else { Some(end) };
    }

    pub fn yank_line(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        let (pulled, end) = self.pull_from_buffer(text, next_line_end);
        self.yank_string(text, &pulled, is_hidden);
        self.yank_prev_point = if self.state.success { None } else { Some(end) };
    }

    fn pull_from_buffer(
        &self,
        text: &str,
        advance: fn(&str, usize) -> usize,
    ) -> (String, ByteOffset) {
        let mut start = if !self.state.forward {
            self.state.other_end.unwrap_or(self.state.point).0
        } else {
            self.state.point.0
        };
        if !self.state.success
            && let Some(prev) = self.yank_prev_point
        {
            start = prev.0;
        }
        start = floor_char_boundary(text, start.min(text.len()));
        let end = advance(text, start);
        (text[start..end].to_owned(), ByteOffset(end))
    }

    /// `C-s` / `C-r` (`isearch-repeat`).
    pub fn repeat(
        &mut self,
        text: &str,
        forward: bool,
        ring: &IsearchRing,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        self.status_hint = None;
        self.yank_prev_point = None;
        let same_direction = self.state.forward == forward;
        if same_direction {
            if self.state.query.is_empty() {
                let regexp_ring = self.state.mode.uses_regexp_ring();
                if let Some(last) = ring.latest(regexp_ring) {
                    self.state.query = last.to_owned();
                    self.state.error = None;
                    self.ring_index = Some(0);
                } else {
                    self.state.error = Some("No previous search string".into());
                    self.push_state();
                    return;
                }
            } else if !self.state.success {
                // `isearch-wrap-pause = t`: second repeat after failure wraps around.
                self.state.wrapped = true;
                self.state.point = if forward {
                    ByteOffset(0)
                } else {
                    ByteOffset(text.len())
                };
            }
        } else {
            // `isearch-repeat-on-direction-change = nil`: flip direction at current point.
            self.state.forward = forward;
            self.state.success = true;
            self.state.error = None;
        }

        self.state.barrier = self.state.point;
        if self.state.query.is_empty() {
            self.state.success = true;
            self.push_state();
            return;
        }

        // If repeating a zero-length match (e.g. `^` or `$`), advance one char first.
        if self.state.success
            && self.state.other_end == Some(self.state.point)
            && !self.state.just_started
        {
            if self.state.forward {
                if self.state.point.0 >= text.len() {
                    self.state.success = false;
                    self.push_state();
                    return;
                }
                self.state.point = ByteOffset(next_char_offset(text, self.state.point.0));
            } else {
                if self.state.point.0 == 0 {
                    self.state.success = false;
                    self.push_state();
                    return;
                }
                self.state.point = ByteOffset(prev_char_offset(text, self.state.point.0));
            }
        }

        self.run_search(text, &is_hidden);
        self.push_state();
    }

    /// `DEL` (`isearch-delete-char`): undo the last isearch input item.
    pub fn delete_char(&mut self) -> bool {
        self.status_hint = None;
        self.yank_prev_point = None;
        self.pop_state()
    }

    /// `C-M-d` (`isearch-del-char`): delete the last character of `query` and re-search.
    pub fn del_char(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) -> bool {
        self.status_hint = None;
        self.yank_prev_point = None;
        if self.state.query.pop().is_none() {
            return false;
        }
        if let Some(other) = self.state.other_end {
            self.state.point = other;
        }
        if self.state.query.is_empty() {
            self.state.success = true;
            self.state.error = None;
            self.state.other_end = None;
        } else {
            self.run_search(text, &is_hidden);
        }
        self.push_state();
        true
    }

    /// `C-g` (`isearch-abort`): two-stage abort.
    pub fn abort(&mut self) -> AbortOutcome {
        self.status_hint = None;
        self.yank_prev_point = None;
        if self.state.success && self.state.error.is_none() {
            self.state.point = self.opoint;
            self.state.success = false;
            return AbortOutcome::Cancelled;
        }
        while (!self.state.success || self.state.error.is_some()) && self.pop_state() {}
        AbortOutcome::Reverted
    }

    /// `M-p` (`isearch-ring-retreat`) / `M-n` (`isearch-ring-advance`).
    pub fn ring_step(
        &mut self,
        text: &str,
        advance: bool,
        ring: &IsearchRing,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> bool {
        self.status_hint = None;
        self.yank_prev_point = None;
        let regexp = self.state.mode.uses_regexp_ring();
        let len = ring.len(regexp);
        if len == 0 {
            return false;
        }
        let next = match self.ring_index {
            None => {
                if advance {
                    len - 1
                } else {
                    0
                }
            }
            Some(current) => {
                if advance {
                    (current + len - 1) % len
                } else {
                    (current + 1) % len
                }
            }
        };
        self.ring_index = Some(next);
        self.state.query = ring.get(regexp, next).unwrap().to_owned();
        self.state.error = None;
        if let Some(other) = self.state.other_end {
            self.state.point = other;
        }
        self.run_search(text, &is_hidden);
        self.push_state();
        true
    }

    /// `M-c` / `M-s c` (`isearch-toggle-case-fold`).
    pub fn toggle_case_fold(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        let next = !self.state.effective_case_fold();
        self.state.case_fold_override = Some(next);
        self.status_hint = Some(
            if next {
                "case insensitive"
            } else {
                "case sensitive"
            }
            .into(),
        );
        self.research_current(text, is_hidden);
    }

    /// `M-r` / `M-s r` (`isearch-toggle-regexp`).
    pub fn toggle_regexp(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        self.state.mode = if self.state.mode == SearchMode::Regexp {
            SearchMode::Literal
        } else {
            SearchMode::Regexp
        };
        self.state.strict_bounds = false;
        self.status_hint = None;
        self.research_current(text, is_hidden);
    }

    /// `M-s w` (`isearch-toggle-word`).
    pub fn toggle_word(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        self.state.mode = if self.state.mode == SearchMode::Word {
            SearchMode::Literal
        } else {
            SearchMode::Word
        };
        self.state.strict_bounds = false;
        self.status_hint = None;
        self.research_current(text, is_hidden);
    }

    /// `M-s _` (`isearch-toggle-symbol`).
    pub fn toggle_symbol(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        self.state.mode = if self.state.mode == SearchMode::Symbol {
            SearchMode::Literal
        } else {
            SearchMode::Symbol
        };
        self.state.strict_bounds = false;
        self.status_hint = None;
        self.research_current(text, is_hidden);
    }

    /// `M-s SPC` (`isearch-toggle-lax-whitespace`).
    pub fn toggle_lax_whitespace(
        &mut self,
        text: &str,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        self.state.lax_whitespace = !self.state.lax_whitespace;
        self.status_hint = Some(
            if self.state.lax_whitespace {
                "lax whitespace"
            } else {
                "strict whitespace"
            }
            .into(),
        );
        self.research_current(text, is_hidden);
    }

    /// `M-s i` (`isearch-toggle-invisible`).
    pub fn toggle_invisible(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        self.state.search_invisible = !self.state.search_invisible;
        self.status_hint = Some(
            if self.state.search_invisible {
                "search invisible"
            } else {
                "skip invisible"
            }
            .into(),
        );
        self.research_current(text, is_hidden);
    }

    /// `M-s M-<` (`isearch-beginning-of-buffer`) / `M-s M->` (`isearch-end-of-buffer`).
    pub fn jump_to_boundary(
        &mut self,
        text: &str,
        beginning: bool,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        self.status_hint = None;
        self.yank_prev_point = None;
        if self.state.query.is_empty() {
            return;
        }
        let original_forward = self.state.forward;
        self.state.just_started = true;
        self.state.forward = beginning;
        self.state.point = if beginning {
            ByteOffset(0)
        } else {
            ByteOffset(text.len())
        };
        self.state.barrier = self.state.point;
        self.state.other_end = None;
        self.state.wrapped = false;
        self.run_search(text, &is_hidden);
        // Preserve original search direction as in Emacs `isearch-beginning-of-buffer`.
        if self.state.success && self.state.forward != original_forward {
            self.state.forward = original_forward;
            if let Some(other) = self.state.other_end {
                let old_point = self.state.point;
                self.state.point = other;
                self.state.other_end = Some(old_point);
            }
        }
        self.push_state();
    }

    /// Applies an edited string from `M-e` (`isearch-edit-string`) or nonincremental search.
    pub fn apply_edited_query(
        &mut self,
        text: &str,
        query: String,
        forward: bool,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        self.status_hint = None;
        self.yank_prev_point = None;
        self.state.query = query;
        self.state.forward = forward;
        self.state.error = None;
        self.state.wrapped = false;
        if self.state.query.is_empty() {
            self.state.success = true;
            self.state.other_end = None;
            self.push_state();
            return;
        }
        if let Some(other) = self.state.other_end {
            self.state.point = other;
        }
        self.state.barrier = self.state.point;
        self.run_search(text, &is_hidden);
        self.push_state();
    }

    fn research_current(&mut self, text: &str, is_hidden: impl Fn(Range<ByteOffset>) -> bool) {
        if self.state.query.is_empty() {
            self.state.success = true;
            self.state.error = None;
            self.push_state();
            return;
        }
        if let Some(other) = self.state.other_end {
            self.state.point = other;
        }
        self.run_search(text, &is_hidden);
        self.push_state();
    }

    fn search_and_update(
        &mut self,
        text: &str,
        yank_flag: bool,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) {
        let should_search = self.state.success || self.state.mode == SearchMode::Regexp;
        if should_search {
            if !self.state.forward {
                // Reverse incremental search: check if text starting at point
                // already matches the extended query without crossing opoint/barrier.
                let limit = self.opoint.min(self.state.barrier).0;
                if let Ok(matcher) = compile_matcher(&self.state)
                    && let Some(m) = matcher.match_at(text, self.state.point.0)
                    && (yank_flag || m.end <= limit)
                    && (self.state.search_invisible
                        || !is_hidden(ByteOffset(m.start)..ByteOffset(m.end)))
                {
                    self.state.success = true;
                    self.state.error = None;
                    self.state.other_end = Some(ByteOffset(m.end));
                    self.state.just_started = false;
                    self.push_state();
                    return;
                }
            }
            if let Some(other) = self.state.other_end {
                self.state.point = if self.state.forward {
                    other
                } else {
                    let bound = self
                        .opoint
                        .0
                        .min(self.state.barrier.0)
                        .min(next_char_offset(text, other.0));
                    ByteOffset(bound)
                };
            }
            self.run_search(text, &is_hidden);
        }
        self.push_state();
    }

    fn run_search(&mut self, text: &str, is_hidden: &impl Fn(Range<ByteOffset>) -> bool) {
        let matcher = match compile_matcher(&self.state) {
            Ok(matcher) => {
                self.state.error = None;
                matcher
            }
            Err(error) => {
                self.state.success = false;
                self.state.error = Some(error);
                if let Some(prev) = self.cmds.last() {
                    self.state.point = prev.point;
                    self.state.other_end = prev.other_end;
                }
                return;
            }
        };

        let skip_hidden =
            |range: Range<ByteOffset>| !self.state.search_invisible && is_hidden(range);
        let found = if self.state.forward {
            matcher.find_forward(text, self.state.point.0, skip_hidden)
        } else {
            matcher.find_backward(text, self.state.point.0, skip_hidden)
        };
        self.state.just_started = false;
        if let Some(range) = found {
            self.state.success = true;
            if self.state.forward {
                self.state.point = ByteOffset(range.end);
                self.state.other_end = Some(ByteOffset(range.start));
            } else {
                self.state.point = ByteOffset(range.start);
                self.state.other_end = Some(ByteOffset(range.end));
            }
        } else {
            self.state.success = false;
            if let Some(prev) = self.cmds.last() {
                self.state.point = prev.point;
                self.state.other_end = prev.other_end;
            }
        }
    }

    /// Computes non-overlapping lazy highlights in `visible_range` excluding the current match.
    pub fn lazy_highlights(
        &self,
        text: &str,
        visible_range: Range<usize>,
        is_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> Vec<Range<ByteOffset>> {
        if self.state.query.is_empty() || self.state.error.is_some() {
            return Vec::new();
        }
        let Ok(matcher) = compile_matcher(&self.state) else {
            return Vec::new();
        };
        let current = self.match_range();
        let start = floor_char_boundary(text, visible_range.start.min(text.len()));
        let end = ceil_char_boundary(text, visible_range.end.min(text.len()));
        if start >= end {
            return Vec::new();
        }
        let mut results = Vec::new();
        let mut pos = start;
        while pos < end && results.len() < LAZY_HIGHLIGHT_LIMIT {
            let Some(m) = matcher.find_forward_bounded(text, pos, end, |r| {
                !self.state.search_invisible && is_hidden(r)
            }) else {
                break;
            };
            if m.start >= end {
                break;
            }
            let range = ByteOffset(m.start)..ByteOffset(m.end);
            if range.start < range.end && current.as_ref() != Some(&range) {
                results.push(range);
            }
            if m.end > pos {
                pos = m.end;
            } else if pos < end {
                pos = next_char_offset(text, pos);
            } else {
                break;
            }
        }
        results
    }

    /// Formats the echo area prompt matching Emacs `isearch-message-prefix` / `suffix`.
    pub fn prompt_message(&self, preedit: Option<&str>, quote_pending: bool) -> String {
        let mut prefix = self.message_prefix(false);
        prefix.push_str(&display_search_string(&self.state.query));
        if let Some(preedit) = preedit
            && !preedit.is_empty()
        {
            prefix.push_str(preedit);
        }
        if quote_pending {
            prefix.push_str("^Q");
        }
        if let Some(error) = &self.state.error {
            prefix.push_str(" [");
            prefix.push_str(error);
            prefix.push(']');
        } else if let Some(hint) = &self.status_hint {
            prefix.push_str(" [");
            prefix.push_str(hint);
            prefix.push(']');
        }
        prefix
    }

    pub fn message_prefix(&self, nonincremental: bool) -> String {
        let mut m = String::new();
        if !self.state.success {
            m.push_str("failing ");
        }
        if self.state.wrapped {
            let overwrapped = if self.state.forward {
                self.state.point > self.opoint
            } else {
                self.state.point < self.opoint
            };
            if overwrapped {
                m.push_str("over");
            }
            m.push_str("wrapped ");
        }
        if !self.state.success && !self.state.effective_case_fold() {
            m.push_str("case-sensitive ");
        }
        match self.state.mode {
            SearchMode::Literal => {}
            SearchMode::Regexp => m.push_str("regexp "),
            SearchMode::Word => m.push_str("word "),
            SearchMode::Symbol => m.push_str("symbol "),
        }
        m.push_str(if nonincremental { "search" } else { "I-search" });
        if !self.state.forward {
            m.push_str(" backward");
        }
        m.push_str(": ");
        capitalize_first(m)
    }
}

struct CompiledMatcher {
    regex: Regex,
    mode: SearchMode,
    left_boundary: bool,
    right_boundary: bool,
    literal_case_sensitive: Option<String>,
}

impl CompiledMatcher {
    fn check_boundaries(&self, text: &str, range: &Range<usize>) -> bool {
        match self.mode {
            SearchMode::Word => {
                if self.left_boundary && preceded_by(text, range.start, char::is_alphanumeric) {
                    return false;
                }
                if self.right_boundary && followed_by(text, range.end, char::is_alphanumeric) {
                    return false;
                }
                true
            }
            SearchMode::Symbol => {
                if self.left_boundary && preceded_by(text, range.start, is_symbol_char) {
                    return false;
                }
                if self.right_boundary && followed_by(text, range.end, is_symbol_char) {
                    return false;
                }
                true
            }
            SearchMode::Literal | SearchMode::Regexp => true,
        }
    }

    /// Checks whether the matcher matches starting exactly at `at` (Emacs `looking-at`).
    fn match_at(&self, text: &str, at: usize) -> Option<Range<usize>> {
        if at > text.len() || !text.is_char_boundary(at) {
            return None;
        }
        let m = self.regex.find_at(text, at)?;
        if m.start() != at {
            return None;
        }
        let range = m.start()..m.end();
        self.check_boundaries(text, &range).then_some(range)
    }

    fn find_forward(
        &self,
        text: &str,
        from: usize,
        skip_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> Option<Range<usize>> {
        let mut pos = floor_char_boundary(text, from.min(text.len()));
        if let Some(literal) = &self.literal_case_sensitive {
            while pos <= text.len() {
                let rel = text[pos..].find(literal.as_str())?;
                let start = pos + rel;
                let end = start + literal.len();
                let range = start..end;
                if !skip_hidden(ByteOffset(start)..ByteOffset(end)) {
                    return Some(range);
                }
                if start >= text.len() {
                    return None;
                }
                pos = next_char_offset(text, start);
            }
            return None;
        }
        while pos <= text.len() {
            let m = self.regex.find_at(text, pos)?;
            let range = m.start()..m.end();
            if self.check_boundaries(text, &range)
                && !skip_hidden(ByteOffset(range.start)..ByteOffset(range.end))
            {
                return Some(range);
            }
            if m.start() >= text.len() {
                return None;
            }
            pos = next_char_offset(text, m.start());
        }
        None
    }

    fn find_forward_bounded(
        &self,
        text: &str,
        from: usize,
        end: usize,
        skip_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> Option<Range<usize>> {
        let mut pos = floor_char_boundary(text, from.min(text.len()));
        let end = ceil_char_boundary(text, end.min(text.len()));
        if pos >= end {
            return None;
        }
        if let Some(literal) = &self.literal_case_sensitive {
            let search_end =
                ceil_char_boundary(text, end.saturating_add(literal.len()).min(text.len()));
            while pos < end {
                let rel = text[pos..search_end].find(literal.as_str())?;
                let start = pos + rel;
                if start >= end {
                    return None;
                }
                let m_end = start + literal.len();
                if !skip_hidden(ByteOffset(start)..ByteOffset(m_end)) {
                    return Some(start..m_end);
                }
                pos = next_char_offset(text, start);
            }
            return None;
        }
        let search_end = ceil_char_boundary(text, end.saturating_add(4096).min(text.len()));
        let window = &text[..search_end];
        while pos < end {
            let m = self.regex.find_at(window, pos)?;
            if m.start() >= end {
                return None;
            }
            let range = m.start()..m.end();
            let valid_at_edge = if range.end == search_end && search_end < text.len() {
                self.regex.find_at(text, range.start).is_some_and(|full| {
                    full.start() == range.start
                        && if range.start == range.end {
                            full.end() == range.end
                        } else {
                            full.end() >= range.end
                        }
                })
            } else {
                true
            };
            if valid_at_edge
                && self.check_boundaries(text, &range)
                && !skip_hidden(ByteOffset(range.start)..ByteOffset(range.end))
            {
                return Some(range);
            }
            if m.start() >= end {
                return None;
            }
            pos = next_char_offset(text, m.start());
        }
        None
    }

    fn find_backward(
        &self,
        text: &str,
        limit: usize,
        skip_hidden: impl Fn(Range<ByteOffset>) -> bool,
    ) -> Option<Range<usize>> {
        let limit = floor_char_boundary(text, limit.min(text.len()));
        if let Some(literal) = &self.literal_case_sensitive {
            let mut end_limit = limit;
            while end_limit >= literal.len() {
                let start = text[..end_limit].rfind(literal.as_str())?;
                let end = start + literal.len();
                if !skip_hidden(ByteOffset(start)..ByteOffset(end)) {
                    return Some(start..end);
                }
                if end == 0 {
                    return None;
                }
                end_limit = prev_char_offset(text, end);
            }
            return None;
        }

        // Scan in expanding line-aligned suffix windows for fast local backward search
        // on large files while remaining exact across multiline patterns.
        let mut window = BACKWARD_CHUNK_BYTES;
        loop {
            let chunk_start = if limit <= window {
                0
            } else {
                line_start_at_or_before(text, limit - window)
            };
            if let Some(found) = self.scan_backward_window(text, chunk_start, limit, &skip_hidden)
                && (chunk_start == 0 || found.start > chunk_start)
            {
                return Some(found);
            }
            if chunk_start == 0 {
                return None;
            }
            window = window.saturating_mul(4);
        }
    }

    fn scan_backward_window(
        &self,
        text: &str,
        start_from: usize,
        limit: usize,
        skip_hidden: &impl Fn(Range<ByteOffset>) -> bool,
    ) -> Option<Range<usize>> {
        let prefix = &text[..limit];
        let mut pos = start_from;
        let mut best: Option<Range<usize>> = None;
        while pos <= limit {
            let Some(m) = self.regex.find_at(prefix, pos) else {
                break;
            };
            let range = m.start()..m.end();
            // Reject matches that only succeed because `prefix` artificially terminates at
            // `limit` (e.g., `foo$`, `foo\b`, `$`), while still allowing greedy matches like
            // `a+` in `"aaaa"` at `limit = 2`.
            let valid_at_limit = if range.end == limit && limit < text.len() {
                self.regex.find_at(text, range.start).is_some_and(|full| {
                    full.start() == range.start
                        && if range.start == range.end {
                            full.end() == range.end
                        } else {
                            full.end() >= range.end
                        }
                })
            } else {
                true
            };
            if valid_at_limit
                && self.check_boundaries(text, &range)
                && !skip_hidden(ByteOffset(range.start)..ByteOffset(range.end))
            {
                best = Some(range.clone());
            }
            if m.start() >= limit {
                break;
            }
            pos = next_char_offset(prefix, m.start());
        }
        best
    }
}

fn compile_matcher(state: &IsearchState) -> Result<CompiledMatcher, String> {
    let case_fold = state.effective_case_fold();
    match state.mode {
        SearchMode::Literal => {
            let has_lax_ws = state.lax_whitespace && state.query.contains([' ', '\t']);
            if !case_fold && !has_lax_ws {
                let regex = RegexBuilder::new(&regex::escape(&state.query))
                    .size_limit(REGEX_SIZE_LIMIT)
                    .build()
                    .map_err(friendly_regex_error)?;
                return Ok(CompiledMatcher {
                    regex,
                    mode: SearchMode::Literal,
                    left_boundary: false,
                    right_boundary: false,
                    literal_case_sensitive: Some(state.query.clone()),
                });
            }
            let pattern = if has_lax_ws {
                lax_whitespace_pattern(&state.query)
            } else {
                regex::escape(&state.query)
            };
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(case_fold)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(friendly_regex_error)?;
            Ok(CompiledMatcher {
                regex,
                mode: SearchMode::Literal,
                left_boundary: false,
                right_boundary: false,
                literal_case_sensitive: None,
            })
        }
        SearchMode::Regexp => {
            let regex = RegexBuilder::new(&state.query)
                .multi_line(true)
                .dot_matches_new_line(false)
                .case_insensitive(case_fold)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(friendly_regex_error)?;
            Ok(CompiledMatcher {
                regex,
                mode: SearchMode::Regexp,
                left_boundary: false,
                right_boundary: false,
                literal_case_sensitive: None,
            })
        }
        SearchMode::Word => {
            let trimmed = state.query.trim();
            let pattern = if trimmed.is_empty() {
                regex::escape(&state.query)
            } else {
                let parts: Vec<String> = trimmed.split_whitespace().map(regex::escape).collect();
                parts.join(r"[^[:alnum:]]+")
            };
            let left_boundary = trimmed.chars().next().is_some_and(char::is_alphanumeric);
            let right_boundary = (state.strict_bounds
                || !state.lax_whitespace
                || state.query.ends_with(char::is_whitespace))
                && trimmed.chars().last().is_some_and(char::is_alphanumeric);
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(case_fold)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(friendly_regex_error)?;
            Ok(CompiledMatcher {
                regex,
                mode: SearchMode::Word,
                left_boundary,
                right_boundary,
                literal_case_sensitive: None,
            })
        }
        SearchMode::Symbol => {
            let trimmed = state.query.trim();
            let pattern = if trimmed.is_empty() {
                regex::escape(&state.query)
            } else {
                regex::escape(trimmed)
            };
            let left_boundary = trimmed.chars().next().is_some_and(is_symbol_char);
            let right_boundary = (state.strict_bounds
                || !state.lax_whitespace
                || state.query.ends_with(char::is_whitespace))
                && trimmed.chars().last().is_some_and(is_symbol_char);
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(case_fold)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(friendly_regex_error)?;
            Ok(CompiledMatcher {
                regex,
                mode: SearchMode::Symbol,
                left_boundary,
                right_boundary,
                literal_case_sensitive: None,
            })
        }
    }
}

fn lax_whitespace_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() * 2);
    let mut chars = query.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, ' ' | '\t') {
            while matches!(chars.peek(), Some(' ' | '\t')) {
                chars.next();
            }
            pattern.push_str(r"[ \t]+");
        } else {
            let mut buf = [0u8; 4];
            pattern.push_str(&regex::escape(ch.encode_utf8(&mut buf)));
        }
    }
    pattern
}

fn friendly_regex_error(error: regex::Error) -> String {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("unclosed")
        || lower.contains("incomplete")
        || lower.contains("unexpected end")
        || lower.contains("expected")
        || lower.contains("unopened")
    {
        "incomplete input".into()
    } else if matches!(error, regex::Error::CompiledTooBig(_)) {
        "regular expression too big".into()
    } else {
        "invalid regexp".into()
    }
}

/// Emacs `isearch-no-upper-case-p`: true when `query` has no unescaped uppercase chars.
pub fn no_upper_case(query: &str, regexp: bool) -> bool {
    let mut escaped = false;
    for ch in query.chars() {
        if regexp && ch == '\\' {
            escaped = !escaped;
            continue;
        }
        if !escaped && ch.is_uppercase() {
            return false;
        }
        escaped = false;
    }
    !(regexp && (query.contains("[:upper:]") || query.contains("[:lower:]")))
}

fn is_symbol_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub fn symbol_bounds_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    let offset = floor_char_boundary(text, offset.min(text.len()));
    let anchor = if offset < text.len() && text[offset..].chars().next().is_some_and(is_symbol_char)
    {
        offset
    } else if offset > 0 {
        let prev = prev_char_offset(text, offset);
        if text[prev..].chars().next().is_some_and(is_symbol_char) {
            prev
        } else {
            return None;
        }
    } else {
        return None;
    };
    let mut start = anchor;
    while start > 0 {
        let prev = prev_char_offset(text, start);
        if text[prev..].chars().next().is_some_and(is_symbol_char) {
            start = prev;
        } else {
            break;
        }
    }
    let mut end = anchor;
    while end < text.len() {
        let ch = text[end..].chars().next().unwrap();
        if is_symbol_char(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    (start < end).then_some((start, end))
}

fn next_word_or_char_end(text: &str, start: usize) -> usize {
    if start >= text.len() {
        return start;
    }
    let mut iter = text[start..].char_indices();
    let (_, first) = iter.next().unwrap();
    let second = iter.next().map(|(_, ch)| ch);
    if first.is_alphanumeric() || second.is_some_and(char::is_alphanumeric) {
        let mut pos = start;
        while pos < text.len() {
            let ch = text[pos..].chars().next().unwrap();
            if ch.is_alphanumeric() {
                break;
            }
            pos += ch.len_utf8();
        }
        while pos < text.len() {
            let ch = text[pos..].chars().next().unwrap();
            if !ch.is_alphanumeric() {
                break;
            }
            pos += ch.len_utf8();
        }
        pos
    } else {
        start + first.len_utf8()
    }
}

fn next_symbol_or_char_end(text: &str, start: usize) -> usize {
    if start >= text.len() {
        return start;
    }
    let mut iter = text[start..].char_indices();
    let (_, first) = iter.next().unwrap();
    let second = iter.next().map(|(_, ch)| ch);
    if is_symbol_char(first) || second.is_some_and(is_symbol_char) {
        let mut pos = start;
        while pos < text.len() {
            let ch = text[pos..].chars().next().unwrap();
            if is_symbol_char(ch) {
                break;
            }
            pos += ch.len_utf8();
        }
        while pos < text.len() {
            let ch = text[pos..].chars().next().unwrap();
            if !is_symbol_char(ch) {
                break;
            }
            pos += ch.len_utf8();
        }
        pos
    } else {
        start + first.len_utf8()
    }
}

fn next_line_end(text: &str, start: usize) -> usize {
    if start >= text.len() {
        return start;
    }
    if text.as_bytes()[start] == b'\n' {
        let after = start + 1;
        return text[after..]
            .find('\n')
            .map_or(text.len(), |rel| after + rel);
    }
    text[start..]
        .find('\n')
        .map_or(text.len(), |rel| start + rel)
}

fn preceded_by(text: &str, offset: usize, pred: fn(char) -> bool) -> bool {
    offset > 0 && text[..offset].chars().next_back().is_some_and(pred)
}

fn followed_by(text: &str, offset: usize, pred: fn(char) -> bool) -> bool {
    offset < text.len() && text[offset..].chars().next().is_some_and(pred)
}

fn clamp_state(state: &mut IsearchState, text: &str) {
    state.point = ByteOffset(floor_char_boundary(text, state.point.0));
    state.barrier = ByteOffset(floor_char_boundary(text, state.barrier.0));
    state.other_end = state
        .other_end
        .map(|offset| ByteOffset(floor_char_boundary(text, offset.0)));
}

fn next_char_offset(text: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(text, offset);
    text[offset..]
        .chars()
        .next()
        .map_or(text.len(), |ch| offset + ch.len_utf8())
}

fn prev_char_offset(text: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(text, offset);
    text[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn line_start_at_or_before(text: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(text, offset);
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn capitalize_first(mut s: String) -> String {
    if let Some(first) = s.chars().next() {
        let upper: String = first.to_uppercase().collect();
        s.replace_range(..first.len_utf8(), &upper);
    }
    s
}

fn display_search_string(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for ch in query.chars() {
        match ch {
            '\n' => out.push_str("^J"),
            '\t' => out.push('\t'),
            c if c.is_control() => {
                let code = c as u32;
                if code < 32 {
                    out.push('^');
                    out.push((code as u8 + b'@') as char);
                } else {
                    out.push(c);
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(_: Range<ByteOffset>) -> bool {
        false
    }

    #[test]
    fn incremental_forward_extends_current_match_and_repeats_with_wrap_pause() {
        let text = "alpha beta alpha gamma";
        let ring = IsearchRing::default();
        let mut engine = IsearchEngine::new(ByteOffset(6), true, SearchMode::Literal);

        engine.append_text(text, "a", visible);
        assert_eq!(engine.match_range(), Some(ByteOffset(9)..ByteOffset(10)));
        assert_eq!(engine.state.point, ByteOffset(10));

        engine.append_text(text, "lpha", visible);
        assert_eq!(engine.match_range(), Some(ByteOffset(11)..ByteOffset(16)));
        assert_eq!(engine.state.point, ByteOffset(16));

        // First repeat past last match fails (wrap pause).
        engine.repeat(text, true, &ring, visible);
        assert!(!engine.state.success);
        assert_eq!(engine.state.point, ByteOffset(16));
        assert!(
            engine
                .prompt_message(None, false)
                .starts_with("Failing I-search: ")
        );

        // Second repeat wraps around to the top (point 5 <= opoint 6 => Wrapped).
        engine.repeat(text, true, &ring, visible);
        assert!(engine.state.success);
        assert!(engine.state.wrapped);
        assert_eq!(engine.match_range(), Some(ByteOffset(0)..ByteOffset(5)));
        assert!(
            engine
                .prompt_message(None, false)
                .starts_with("Wrapped I-search: ")
        );

        // Repeating past opoint (point 16 > opoint 6 => Overwrapped).
        engine.repeat(text, true, &ring, visible);
        assert!(engine.state.success);
        assert_eq!(engine.match_range(), Some(ByteOffset(11)..ByteOffset(16)));
        assert!(
            engine
                .prompt_message(None, false)
                .starts_with("Overwrapped I-search: ")
        );
    }

    #[test]
    fn backward_incremental_and_direction_reversal_match_emacs() {
        let text = "aaa foo bar foo baz";
        let ring = IsearchRing::default();
        let mut engine = IsearchEngine::new(ByteOffset(text.len()), false, SearchMode::Literal);

        engine.append_text(text, "f", visible);
        assert_eq!(engine.match_range(), Some(ByteOffset(12)..ByteOffset(13)));
        assert_eq!(engine.state.point, ByteOffset(12));

        // Extending backward search stays at byte 12 because "foo" fits before opoint.
        engine.append_text(text, "oo", visible);
        assert_eq!(engine.match_range(), Some(ByteOffset(12)..ByteOffset(15)));
        assert_eq!(engine.state.point, ByteOffset(12));

        // Reversing direction (C-s) stays on the same match, moving point to its end.
        engine.repeat(text, true, &ring, visible);
        assert!(engine.state.forward);
        assert_eq!(engine.match_range(), Some(ByteOffset(12)..ByteOffset(15)));
        assert_eq!(engine.state.point, ByteOffset(15));

        // Overlapping backward literal match ("aa" in "aaa") finds rightmost 1..3.
        let mut overlap = IsearchEngine::new(ByteOffset(4), false, SearchMode::Literal);
        overlap.append_text(text, "aa", visible);
        assert_eq!(overlap.match_range(), Some(ByteOffset(1)..ByteOffset(3)));
    }

    #[test]
    fn smart_case_and_yank_downcasing_and_toggle() {
        let text = "Foo foo FOO";
        let mut engine = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);

        engine.append_text(text, "foo", visible);
        assert!(engine.state.effective_case_fold());
        assert_eq!(engine.match_range(), Some(ByteOffset(0)..ByteOffset(3)));

        // Typing uppercase flips smart-case to case-sensitive.
        let mut sensitive = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);
        sensitive.append_text(text, "FOO", visible);
        assert!(!sensitive.state.effective_case_fold());
        assert_eq!(sensitive.match_range(), Some(ByteOffset(8)..ByteOffset(11)));

        // Explicit M-c toggles back to case-insensitive.
        sensitive.toggle_case_fold(text, visible);
        assert!(sensitive.state.effective_case_fold());
        assert_eq!(sensitive.status_hint.as_deref(), Some("case insensitive"));

        // Yanked uppercase is downcased when case-folding (`search-upper-case = not-yanks`).
        let mut yanked = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);
        yanked.yank_word_or_char(text, visible);
        assert_eq!(yanked.state.query, "foo");
        assert!(yanked.state.effective_case_fold());
    }

    #[test]
    fn regexp_incomplete_recovers_and_zero_length_advances() {
        let text = "alpha\nbeta\ngamma";
        let ring = IsearchRing::default();
        let mut engine = IsearchEngine::new(ByteOffset(0), true, SearchMode::Regexp);

        engine.append_text(text, "(be", visible);
        assert!(!engine.state.success);
        assert_eq!(engine.state.error.as_deref(), Some("incomplete input"));

        // Completing the regexp recovers immediately.
        engine.append_text(text, "ta)", visible);
        assert!(engine.state.success);
        assert!(engine.state.error.is_none());
        assert_eq!(engine.match_range(), Some(ByteOffset(6)..ByteOffset(10)));

        // Zero-length `^` advances line by line without getting stuck.
        let mut anchors = IsearchEngine::new(ByteOffset(0), true, SearchMode::Regexp);
        anchors.append_text(text, "^", visible);
        assert_eq!(anchors.state.point, ByteOffset(0));
        anchors.repeat(text, true, &ring, visible);
        assert_eq!(anchors.state.point, ByteOffset(6));
        anchors.repeat(text, true, &ring, visible);
        assert_eq!(anchors.state.point, ByteOffset(11));
    }

    #[test]
    fn two_stage_abort_and_delete_char_stack() {
        let text = "hello world";
        let mut engine = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);

        engine.append_text(text, "hel", visible);
        assert!(engine.state.success);
        engine.append_text(text, "z", visible);
        assert!(!engine.state.success);
        engine.append_text(text, "q", visible);
        assert!(!engine.state.success);

        // First C-g reverts to last successful state ("hel") and stays in isearch.
        assert_eq!(engine.abort(), AbortOutcome::Reverted);
        assert_eq!(engine.state.query, "hel");
        assert!(engine.state.success);

        // Second C-g cancels completely and returns to opoint.
        assert_eq!(engine.abort(), AbortOutcome::Cancelled);
        assert_eq!(engine.state.point, ByteOffset(0));
    }

    #[test]
    fn lax_whitespace_and_word_symbol_modes() {
        let text = "foo   \tbar foobar foo_bar";
        let mut lax = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);
        lax.append_text(text, "foo bar", visible);
        assert_eq!(lax.match_range(), Some(ByteOffset(0)..ByteOffset(10)));

        lax.toggle_lax_whitespace(text, visible);
        assert!(!lax.state.success);

        let mut sym = IsearchEngine::start_symbol_at_point(ByteOffset(1), text, visible);
        assert_eq!(sym.state.query, "foo");
        assert_eq!(sym.match_range(), Some(ByteOffset(0)..ByteOffset(3)));
        let ring = IsearchRing::default();
        // Neither "foobar" nor "foo_bar" matches strict symbol "foo".
        sym.repeat(text, true, &ring, visible);
        assert!(!sym.state.success);
    }

    #[test]
    fn hidden_filter_skips_hidden_matches_when_search_invisible_disabled() {
        let text = "visible\nhidden_match\nvisible_match";
        let hidden = ByteOffset(8)..ByteOffset(21);
        let is_hidden = |r: Range<ByteOffset>| r.start < hidden.end && r.end > hidden.start;

        let mut engine = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);
        engine.append_text(text, "match", is_hidden);
        // Default search_invisible = true finds the hidden match.
        assert_eq!(engine.match_range(), Some(ByteOffset(15)..ByteOffset(20)));

        // Toggling M-s i skips hidden sections and lands on visible_match.
        engine.toggle_invisible(text, is_hidden);
        assert!(!engine.state.search_invisible);
        assert_eq!(engine.match_range(), Some(ByteOffset(29)..ByteOffset(34)));
    }

    #[test]
    fn backward_regex_rejects_synthetic_midline_anchors_and_allows_greedy_prefix() {
        let mut mid = IsearchEngine::new(ByteOffset(3), false, SearchMode::Regexp);
        mid.append_text("foobar", "foo$", visible);
        assert!(!mid.state.success);

        let mut mid_word = IsearchEngine::new(ByteOffset(3), false, SearchMode::Regexp);
        mid_word.append_text("foobar", r"foo\b", visible);
        assert!(!mid_word.state.success);

        let mut real_word = IsearchEngine::new(ByteOffset(3), false, SearchMode::Regexp);
        real_word.append_text("foo bar", r"foo\b", visible);
        assert_eq!(real_word.match_range(), Some(ByteOffset(0)..ByteOffset(3)));

        let mut greedy = IsearchEngine::new(ByteOffset(2), false, SearchMode::Regexp);
        greedy.append_text("aaaa", "a+", visible);
        assert_eq!(greedy.match_range(), Some(ByteOffset(1)..ByteOffset(2)));
    }

    #[test]
    fn symbol_at_point_on_whitespace_stays_searchable_and_lax_tab_and_clamp() {
        let text = "   foo\t\tbar";
        let mut engine = IsearchEngine::start_symbol_at_point(ByteOffset(1), text, visible);
        assert!(engine.state.success);
        assert_eq!(engine.state.error.as_deref(), Some("No symbol at point"));
        engine.append_text(text, "foo", visible);
        assert!(engine.state.success);
        assert_eq!(engine.match_range(), Some(ByteOffset(3)..ByteOffset(6)));

        // Tab in query acts as lax whitespace matching spaces/tabs.
        let mut lax_tab = IsearchEngine::new(ByteOffset(0), true, SearchMode::Literal);
        lax_tab.append_text("foo   bar", "foo\tbar", visible);
        assert_eq!(lax_tab.match_range(), Some(ByteOffset(0)..ByteOffset(9)));

        // Clamping after out-of-band truncation keeps offsets at valid UTF-8 boundaries.
        engine.clamp_to_text("你");
        assert!(engine.state.point.0 <= "你".len());
        assert!("你".is_char_boundary(engine.state.point.0));
    }
}
