use super::Buffer;
use crate::status::{RenderedStatus, Repository, Status, StatusLocation};
use std::sync::Arc;

pub(super) struct StatusSession {
    repository: Repository,
    request: u64,
    data: Option<Arc<Status>>,
    locations: Vec<StatusLocation>,
    stale: bool,
}

impl Buffer {
    pub(crate) fn status(repository: Repository) -> Self {
        let buffer = Self::named(&format!(
            "*magit-status: {}*",
            repository.root.to_string_lossy().escape_debug()
        ));
        {
            let mut doc = buffer.document.borrow_mut();
            doc.mode = crate::mode::ModeId::MAGIT_STATUS;
            doc.read_only = true;
            doc.status = Some(StatusSession {
                repository,
                request: 0,
                data: None,
                locations: Vec::new(),
                stale: false,
            });
        }
        buffer
    }

    pub fn status_data(&self) -> Option<Arc<Status>> {
        self.document.borrow().status.as_ref()?.data.clone()
    }

    pub(crate) fn begin_status_request(&mut self) -> Result<(Repository, u64), String> {
        let mut doc = self.document.borrow_mut();
        let session = doc.status.as_mut().ok_or("Not a status buffer")?;
        session.request = session
            .request
            .checked_add(1)
            .expect("status request identity exhausted");
        Ok((session.repository.clone(), session.request))
    }

    pub(crate) fn status_repository(&self) -> Option<Repository> {
        self.document
            .borrow()
            .status
            .as_ref()
            .map(|session| session.repository.clone())
    }

    pub(crate) fn status_request_matches(&self, request: u64, repository: &Repository) -> bool {
        self.document
            .borrow()
            .status
            .as_ref()
            .is_some_and(|session| session.request == request && &session.repository == repository)
    }

    pub(crate) fn install_status(
        &mut self,
        request: u64,
        data: Status,
        rendered: RenderedStatus,
    ) -> bool {
        if !self.status_request_matches(request, data.repository()) {
            return false;
        }
        self.install_projection(&rendered.text, rendered.sections, rendered.styles);
        let mut doc = self.document.borrow_mut();
        let session = doc.status.as_mut().unwrap();
        session.data = Some(Arc::new(data));
        session.locations = rendered.locations;
        session.stale = false;
        true
    }

    pub(crate) fn status_identity(&self) -> Option<(Repository, u64)> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref()?;
        Some((session.repository.clone(), session.request))
    }

    pub(crate) fn status_selection(&self) -> Option<(StatusLocation, u64)> {
        if self.git_is_stale() || self.region().is_some() {
            return None;
        }
        self.status_leaf()
    }

    pub(crate) fn status_leaf(&self) -> Option<(StatusLocation, u64)> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref()?;
        let point = self.point();
        if doc.sections.as_ref()?.collapse_point(point) != point {
            return None;
        }
        session
            .locations
            .iter()
            .find(|location| location.range.contains(&point))
            .cloned()
            .map(|location| (location, session.request))
    }

    pub fn git_is_stale(&self) -> bool {
        let doc = self.document.borrow();
        doc.status.as_ref().is_some_and(|session| session.stale)
            || doc.diff.as_ref().is_some_and(|session| session.stale)
            || doc
                .repository_history
                .as_ref()
                .is_some_and(|session| session.stale)
    }

    pub(crate) fn mark_git_stale(&mut self) {
        let mut doc = self.document.borrow_mut();
        if let Some(session) = &mut doc.status {
            session.stale = true;
        }
        if let Some(session) = &mut doc.diff {
            session.stale = true;
        }
        if let Some(session) = &mut doc.repository_history
            && matches!(session.query, crate::history::Query::Log { .. })
        {
            session.stale = true;
        }
        doc.touch();
    }

    pub(crate) fn detach_status(&mut self) {
        let mut doc = self.document.borrow_mut();
        if doc.status.take().is_some() {
            doc.sections = None;
            doc.named_styles = Arc::from([]);
            doc.mode = crate::mode::ModeId::FUNDAMENTAL;
            doc.read_only = false;
            doc.touch();
        }
    }
}
