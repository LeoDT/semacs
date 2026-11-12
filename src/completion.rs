//! Candidate filtering and selection, independent of minibuffer text editing.

use std::cmp::Reverse;
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{Atom, AtomKind, CaseMatching, Normalization},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub value: String,
    pub label: String,
    pub annotation: String,
    pub directory: bool,
}

impl Candidate {
    pub fn named(name: String, annotation: String) -> Self {
        Self {
            value: name.clone(),
            label: name,
            annotation,
            directory: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedCandidate {
    pub candidate: Candidate,
    pub highlights: Vec<Range<usize>>,
}

#[derive(Clone, Copy)]
pub(crate) enum CandidateAction {
    Insert,
    Accept,
    Raw,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CandidateResolution {
    Pending,
    Insert(String),
    Accept(String),
    NoMatch,
}

#[derive(Clone, Debug, Default)]
pub struct CandidateList {
    pub items: Arc<Vec<MatchedCandidate>>,
    pub selected: Option<usize>,
    pub loading: bool,
    pub directory_input: bool,
    pub raw_directory: bool,
}

impl CandidateList {
    pub fn ready(
        items: Vec<MatchedCandidate>,
        input: &str,
        default: Option<&str>,
        directory_input: bool,
    ) -> Self {
        let selected = if directory_input {
            None
        } else {
            items
                .iter()
                .position(|item| item.candidate.value == input)
                .or_else(|| {
                    if input.is_empty() {
                        items
                            .iter()
                            .position(|item| Some(item.candidate.value.as_str()) == default)
                    } else {
                        None
                    }
                })
                .or_else(|| (!items.is_empty()).then_some(0))
        };
        Self {
            items: Arc::new(items),
            selected,
            loading: false,
            directory_input,
            raw_directory: directory_input,
        }
    }

    pub(crate) fn resolve(&self, action: CandidateAction, input: &str) -> CandidateResolution {
        use CandidateResolution::*;
        if self.loading {
            return Pending;
        }
        let candidate = match action {
            CandidateAction::Raw => None,
            CandidateAction::Insert => self
                .selected_candidate()
                .or_else(|| self.items.first().map(|item| &item.candidate)),
            CandidateAction::Accept => self.selected_candidate(),
        };
        if let Some(candidate) = candidate {
            return if candidate.directory || matches!(action, CandidateAction::Insert) {
                Insert(candidate.value.clone())
            } else {
                Accept(candidate.value.clone())
            };
        }
        if matches!(action, CandidateAction::Insert) {
            return NoMatch;
        }
        if self.raw_directory {
            if matches!(action, CandidateAction::Accept)
                && !input.is_empty()
                && !input.ends_with(std::path::MAIN_SEPARATOR)
            {
                return Insert(format!("{input}/"));
            }
            // The confirm step owns what a directory answer means: a directory
            // keeps `find-file` open and appends the buffer name for
            // `write-file`.
            return Accept(input.to_owned());
        }
        Accept(input.to_owned())
    }

    pub fn navigate(&mut self, forward: bool) {
        if self.loading {
            return;
        }
        self.selected = match (self.selected, forward) {
            (None, true) if !self.items.is_empty() => Some(0),
            (Some(index), true) => Some((index + 1).min(self.items.len() - 1)),
            (Some(0), false) => None,
            (Some(index), false) => Some(index - 1),
            (selected, _) => selected,
        };
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.items.get(self.selected?).map(|item| &item.candidate)
    }

    pub fn visible_range(&self, rows: usize) -> Range<usize> {
        let end = self
            .items
            .len()
            .min(self.selected.unwrap_or(0).saturating_add(1).max(rows));
        end.saturating_sub(rows)..end
    }
}

pub fn filter(
    candidates: Vec<Candidate>,
    query: &str,
    cancelled: &AtomicBool,
) -> Vec<MatchedCandidate> {
    let atom = Atom::new(
        query,
        CaseMatching::Smart,
        Normalization::Never,
        AtomKind::Fuzzy,
        false,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buffer = Vec::new();
    let mut indices = Vec::new();
    let mut matches = Vec::new();
    for candidate in candidates {
        if cancelled.load(Ordering::Relaxed) {
            return Vec::new();
        }
        indices.clear();
        let Some(score) = atom.indices(
            Utf32Str::new(&candidate.label, &mut buffer),
            &mut matcher,
            &mut indices,
        ) else {
            continue;
        };
        let highlights = candidate
            .label
            .grapheme_indices(true)
            .enumerate()
            .filter(|(index, _)| indices.binary_search(&(*index as u32)).is_ok())
            .map(|(_, (start, text))| start..start + text.len())
            .collect();
        matches.push((
            candidate.label == query,
            score,
            MatchedCandidate {
                candidate,
                highlights,
            },
        ));
    }
    matches.sort_by(|a, b| {
        (
            Reverse(a.0),
            Reverse(a.1),
            &a.2.candidate.label,
            &a.2.candidate.value,
        )
            .cmp(&(
                Reverse(b.0),
                Reverse(b.1),
                &b.2.candidate.label,
                &b.2.candidate.value,
            ))
    });
    matches.into_iter().map(|(_, _, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(names: &[&str], query: &str) -> Vec<MatchedCandidate> {
        filter(
            names
                .iter()
                .map(|name| Candidate::named((*name).into(), "ignored".into()))
                .collect(),
            query,
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn fuzzy_matching_is_smart_case_exact_first_and_does_not_parse_query_syntax() {
        let items = matches(
            &["save-buffer", "sb", "switch-to-buffer", "SAVE-BUFFER"],
            "sb",
        );
        assert_eq!(items[0].candidate.value, "sb");
        assert_eq!(items.len(), 4);
        assert_eq!(matches(&["save-buffer", "SAVE-BUFFER"], "SB").len(), 1);
        assert!(matches(&["save-buffer"], "buffer save").is_empty());
        assert!(matches(&["save-buffer"], "^save").is_empty());
        assert_eq!(matches(&["a b"], "a b").len(), 1);
        assert!(matches(&["anything"], "ignored").is_empty());
    }

    #[test]
    fn highlights_use_utf8_grapheme_ranges() {
        let items = matches(&["你e\u{301}🙂好"], "你🙂");
        assert_eq!(items[0].highlights, vec![0..3, 6..10]);
        let items = matches(&["e\u{301}clair"], "e");
        assert_eq!(items[0].highlights, vec![0..3]);
    }

    #[test]
    fn empty_input_selection_defaults_and_navigation_are_separate_from_text() {
        let items = matches(&["beta", "alpha"], "");
        let mut list = CandidateList::ready(items, "", Some("beta"), false);
        assert_eq!(list.selected_candidate().unwrap().value, "beta");
        list.navigate(true);
        assert_eq!(list.selected, Some(1));
        list.navigate(false);
        list.navigate(false);
        assert_eq!(list.selected, None);
        list.navigate(false);
        assert_eq!(list.selected, None);
        list.navigate(true);
        assert_eq!(list.selected, Some(0));
        assert_eq!(list.visible_range(1), 0..1);
        let list = CandidateList::ready(matches(&["file"], ""), "dir/", None, true);
        assert!(list.selected.is_none());
        assert_eq!(list.visible_range(8), 0..1);
    }

    #[test]
    fn acceptance_distinguishes_selected_candidate_raw_input_and_directories() {
        use CandidateAction as Action;
        use CandidateResolution as Resolution;
        let mut list = CandidateList::ready(matches(&["alpha.txt"], "al"), "al", None, false);
        assert_eq!(
            list.resolve(Action::Accept, "al"),
            Resolution::Accept("alpha.txt".into())
        );
        assert_eq!(
            list.resolve(Action::Raw, "al"),
            Resolution::Accept("al".into())
        );
        list.navigate(false);
        assert_eq!(
            list.resolve(Action::Accept, "al"),
            Resolution::Accept("al".into())
        );
        assert_eq!(
            list.resolve(Action::Insert, "al"),
            Resolution::Insert("alpha.txt".into())
        );
        list.loading = true;
        assert_eq!(list.resolve(Action::Accept, "al"), Resolution::Pending);
        let list = CandidateList::ready(Vec::new(), "dir/", None, true);
        assert_eq!(
            list.resolve(Action::Accept, "dir/"),
            Resolution::Accept("dir/".into())
        );
        assert_eq!(
            list.resolve(Action::Raw, "dir/"),
            Resolution::Accept("dir/".into())
        );
        assert_eq!(list.resolve(Action::Insert, "dir/"), Resolution::NoMatch);
        let mut list = CandidateList::ready(Vec::new(), "dir", None, false);
        list.raw_directory = true;
        assert_eq!(
            list.resolve(Action::Accept, "dir"),
            Resolution::Insert("dir/".into())
        );
        let list = CandidateList::ready(Vec::new(), "new", None, false);
        assert_eq!(
            list.resolve(Action::Accept, "new"),
            Resolution::Accept("new".into())
        );
    }

    #[test]
    fn visible_candidates_follow_the_selection_without_discarding_matches() {
        let items = matches(
            &["00", "01", "02", "03", "04", "05", "06", "07", "08", "09"],
            "",
        );
        let mut list = CandidateList::ready(items, "", None, false);
        for _ in 0..20 {
            list.navigate(true);
        }
        assert_eq!(list.selected, Some(9));
        assert_eq!(list.items.len(), 10);
        assert_eq!(list.visible_range(8), 2..10);
        assert!(list.visible_range(0).is_empty());
        let mut empty = CandidateList::default();
        empty.navigate(true);
        empty.navigate(false);
        assert!(empty.selected.is_none());
    }

    #[test]
    fn cancelled_work_returns_no_results() {
        assert!(
            filter(
                vec![Candidate::named("name".into(), String::new())],
                "",
                &AtomicBool::new(true)
            )
            .is_empty()
        );
    }
}
