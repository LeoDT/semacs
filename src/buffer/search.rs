use std::path::PathBuf;
use std::sync::Arc;

use super::Buffer;
use crate::search::{MatchLocation, RenderedSearch, SearchNavigation, SearchQuery};

pub(super) struct SearchSession {
    query: SearchQuery,
    request: u64,
    navigation: SearchNavigation,
}

impl Buffer {
    pub(crate) fn search(query: SearchQuery) -> Self {
        let buffer = Self::named("*rg*");
        {
            let mut doc = buffer.document.borrow_mut();
            doc.mode = crate::mode::ModeId::RG;
            doc.read_only = true;
            doc.search = Some(SearchSession {
                query,
                request: 0,
                navigation: SearchNavigation::default(),
            });
        }
        buffer
    }

    pub(crate) fn is_search_buffer(&self) -> bool {
        self.document.borrow().search.is_some()
    }

    pub(crate) fn search_query(&self) -> Option<SearchQuery> {
        self.document
            .borrow()
            .search
            .as_ref()
            .map(|session| session.query.clone())
    }

    pub(crate) fn search_directory(&self) -> Option<PathBuf> {
        let doc = self.document.borrow();
        let session = doc.search.as_ref()?;
        Some(session.query.directory.clone())
    }

    pub(crate) fn begin_search_request(&mut self, query: SearchQuery) -> Result<u64, String> {
        let mut doc = self.document.borrow_mut();
        let session = doc.search.as_mut().ok_or("Not a search buffer")?;
        session.request = session
            .request
            .checked_add(1)
            .expect("search request identity exhausted");
        session.query = query;
        Ok(session.request)
    }

    pub(crate) fn search_request_matches(&self, request: u64) -> bool {
        self.document
            .borrow()
            .search
            .as_ref()
            .is_some_and(|session| session.request == request)
    }

    pub(crate) fn install_search(
        &mut self,
        request: u64,
        query: SearchQuery,
        rendered: RenderedSearch,
        preserve_collapse: bool,
    ) -> bool {
        if !self.search_request_matches(request) {
            return false;
        }
        if preserve_collapse {
            self.install_projection(&rendered.text, rendered.sections, rendered.styles);
        } else {
            self.replace_projection(&rendered.text, rendered.sections, rendered.styles);
        }
        let mut doc = self.document.borrow_mut();
        if let Some(session) = doc.search.as_mut() {
            session.query = query;
            session.navigation = rendered.navigation;
        }
        true
    }

    pub(crate) fn search_match_at_point(&self) -> Result<MatchLocation, String> {
        let doc = self.document.borrow();
        let session = doc.search.as_ref().ok_or("Not a search buffer")?;
        session.navigation.match_at(self.point())
    }

    pub(crate) fn search_step_match(&self, forward: bool) -> Result<MatchLocation, String> {
        let doc = self.document.borrow();
        let session = doc.search.as_ref().ok_or("Not a search buffer")?;
        session.navigation.step_match(self.point(), forward)
    }

    pub(crate) fn search_step_file(&self, forward: bool) -> Result<MatchLocation, String> {
        let doc = self.document.borrow();
        let session = doc.search.as_ref().ok_or("Not a search buffer")?;
        session.navigation.step_file(self.point(), forward)
    }

    pub(crate) fn detach_search(&mut self) -> bool {
        let mut doc = self.document.borrow_mut();
        if doc.search.take().is_some() {
            doc.sections = None;
            doc.named_styles = Arc::from([]);
            doc.mode = crate::mode::ModeId::FUNDAMENTAL;
            doc.read_only = false;
            doc.touch();
            true
        } else {
            false
        }
    }

    /// Extracts an identifier-like symbol under or immediately before point
    /// (matching `thing-at-point 'symbol` for search defaults).
    pub(crate) fn symbol_at_point(&self) -> Option<String> {
        let doc = self.document.borrow();
        if doc.sections.is_some() {
            return None;
        }
        let text = &doc.snapshot.text;
        let len = text.len();
        if len == 0 {
            return None;
        }
        let point = self.point().0.min(len);
        let row = text.offset_to_point(point).row;
        let line_start = text.point_to_offset(crate::text::Point::new(row, 0));
        let line_len = text.line_len(row) as usize;
        if line_len == 0 {
            return None;
        }
        let line: String = text
            .chunks_in_range(line_start..line_start + line_len)
            .collect();
        let col = point.saturating_sub(line_start).min(line.len());
        let is_sym = |ch: char| ch.is_alphanumeric() || ch == '_';

        let anchor = if col < line.len() && line[col..].chars().next().is_some_and(is_sym) {
            col
        } else if col > 0 {
            let prev = line[..col].char_indices().next_back()?;
            if is_sym(prev.1) {
                prev.0
            } else {
                return None;
            }
        } else {
            return None;
        };

        let start = line[..anchor]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| is_sym(*ch))
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(anchor);
        let end = line[anchor..]
            .char_indices()
            .take_while(|(_, ch)| is_sym(*ch))
            .last()
            .map(|(idx, ch)| anchor + idx + ch.len_utf8())
            .unwrap_or(anchor);
        let sym = &line[start..end];
        (!sym.is_empty() && sym.len() <= 256).then(|| sym.to_owned())
    }
}
