use super::Buffer;
use crate::history::{Action, Data, Location, Query, RenderedHistory};
use crate::mode::ModeId;
use std::sync::Arc;

pub(super) struct HistorySession {
    pub(super) query: Query,
    requested: Query,
    request: u64,
    data: Option<Arc<Data>>,
    locations: Vec<Location>,
    pub(super) stale: bool,
}

impl Buffer {
    pub(crate) fn history(query: Query) -> Self {
        let buffer = Self::named(&format!(
            "*{}: {}*",
            match query {
                Query::Log { select: true, .. } => "magit-select",
                Query::Log {
                    environment: crate::git::ReadEnvironment::Commit,
                    ..
                } => "magit-log [commit]",
                Query::Log { .. } => "magit-log",
                Query::Revision {
                    environment: crate::git::ReadEnvironment::Commit,
                    ..
                } => "magit-revision [commit]",
                Query::Revision { .. } => "magit-revision",
            },
            query.repository().root.to_string_lossy().escape_debug()
        ));
        let mut doc = buffer.document.borrow_mut();
        doc.mode = match query {
            Query::Log { select: true, .. } => ModeId::MAGIT_LOG_SELECT,
            Query::Log { .. } => ModeId::MAGIT_LOG,
            Query::Revision { .. } => ModeId::MAGIT_REVISION,
        };
        doc.read_only = true;
        doc.repository_history = Some(HistorySession {
            requested: query.clone(),
            query,
            request: 0,
            data: None,
            locations: Vec::new(),
            stale: false,
        });
        drop(doc);
        buffer
    }

    pub(crate) fn history_query(&self) -> Option<Query> {
        self.document
            .borrow()
            .repository_history
            .as_ref()
            .map(|session| session.query.clone())
    }

    pub(crate) fn requested_history_query(&self) -> Option<Query> {
        self.document
            .borrow()
            .repository_history
            .as_ref()
            .map(|session| session.requested.clone())
    }

    pub(crate) fn begin_history_request(&mut self, query: Query) -> Result<u64, String> {
        let mut doc = self.document.borrow_mut();
        let session = doc
            .repository_history
            .as_mut()
            .ok_or("Not a history buffer")?;
        if !session.query.same_buffer(&query) {
            return Err("Wrong history target".into());
        }
        session.requested = query;
        session.request = session
            .request
            .checked_add(1)
            .expect("history request identity exhausted");
        Ok(session.request)
    }

    pub(crate) fn history_request_matches(&self, query: &Query, request: u64) -> bool {
        self.document
            .borrow()
            .repository_history
            .as_ref()
            .is_some_and(|session| session.requested == *query && session.request == request)
    }

    pub(crate) fn install_history(
        &mut self,
        query: &Query,
        request: u64,
        data: Data,
        rendered: RenderedHistory,
    ) -> bool {
        if !self.history_request_matches(query, request) {
            return false;
        }
        self.install_projection(&rendered.text, rendered.sections, rendered.styles);
        let mut doc = self.document.borrow_mut();
        let session = doc.repository_history.as_mut().unwrap();
        session.query = query.clone();
        session.data = Some(Arc::new(data));
        session.locations = rendered.locations;
        session.stale = false;
        true
    }

    pub(crate) fn history_action(&self) -> Result<Action, String> {
        let doc = self.document.borrow();
        let session = doc
            .repository_history
            .as_ref()
            .ok_or("Not a history buffer")?;
        let point = self.point();
        let index = session
            .locations
            .partition_point(|location| location.range.end <= point);
        session
            .locations
            .get(index)
            .filter(|location| location.range.contains(&point))
            .ok_or("No commit or file version here")?
            .action_at(
                session.data.as_deref().ok_or("History is not ready")?,
                &session.query,
                point,
            )
    }

    pub(crate) fn history_commit(&self) -> Option<String> {
        match self.history_action().ok() {
            Some(Action::Commit(oid)) => Some(oid),
            _ => match self.history_query()? {
                Query::Revision { oid, .. } => Some(oid),
                _ => None,
            },
        }
    }

    pub(crate) fn history_commit_point(
        &self,
        oid: Option<&str>,
        skip_fixups: bool,
    ) -> Option<crate::text::ByteOffset> {
        let doc = self.document.borrow();
        let session = doc.repository_history.as_ref()?;
        let Data::Log(log) = session.data.as_deref()? else {
            return None;
        };
        let candidate = oid.map(str::to_owned).or_else(|| {
            log.entries
                .iter()
                .filter_map(|entry| entry.commit.as_ref())
                .find(|commit| {
                    !skip_fixups
                        || !["fixup!", "squash!", "amend!"]
                            .iter()
                            .any(|prefix| commit.message.starts_with(prefix))
                })
                .map(|commit| commit.oid.clone())
        })?;
        session
            .locations
            .iter()
            .find(|location| location.commit_oid() == Some(candidate.as_str()))
            .map(|location| location.range.start)
    }

    pub(crate) fn detach_history(&mut self) {
        let mut doc = self.document.borrow_mut();
        if doc.repository_history.take().is_some() {
            doc.sections = None;
            doc.named_styles = Arc::from([]);
            doc.mode = ModeId::FUNDAMENTAL;
            doc.read_only = false;
            doc.touch();
        }
    }
}
