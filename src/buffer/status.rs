use super::Buffer;
use crate::status::{
    RenderedStatus, Repository, Status, StatusAction, StatusLocation, StatusPreviewTarget,
    StatusVisitSpan,
};
use std::sync::Arc;

pub(super) struct StatusSession {
    repository: Repository,
    request: u64,
    data: Option<Arc<Status>>,
    locations: Vec<StatusLocation>,
    visits: Vec<StatusVisitSpan>,
    copy_spans: Vec<(std::ops::Range<crate::text::ByteOffset>, String)>,
    stale: bool,
}

impl Buffer {
    pub(crate) fn status(repository: Repository) -> Self {
        let dir_name = repository
            .root
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().escape_debug().to_string())
            .unwrap_or_else(|| repository.root.to_string_lossy().escape_debug().to_string());
        let buffer = Self::named(&format!("magit: {dir_name}"));
        {
            let mut doc = buffer.document.borrow_mut();
            doc.mode = crate::mode::ModeId::MAGIT_STATUS;
            doc.read_only = true;
            doc.status = Some(StatusSession {
                repository,
                request: 0,
                data: None,
                locations: Vec::new(),
                visits: Vec::new(),
                copy_spans: Vec::new(),
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
        let first_load = self.sections().is_none();
        let initial_point = rendered.initial_point;
        let copy_spans = rendered.copy_spans;
        self.install_projection(&rendered.text, rendered.sections, rendered.styles);
        let mut doc = self.document.borrow_mut();
        if first_load {
            for point in doc.points.iter().filter_map(|point| point.upgrade()) {
                point.set(initial_point.0);
            }
        }
        let session = doc.status.as_mut().unwrap();
        session.data = Some(Arc::new(data));
        session.locations = rendered.locations;
        session.visits = rendered.visits;
        session.copy_spans = copy_spans;
        session.stale = false;
        true
    }

    pub(crate) fn status_visit(&self, thing: bool) -> Result<StatusAction, String> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref().ok_or("Not a status buffer")?;
        let data = session.data.as_ref().ok_or("Status is not ready")?;
        let point = self.point();
        if doc
            .sections
            .as_ref()
            .is_some_and(|sections| sections.collapse_point(point) != point)
        {
            return Err(if thing {
                "There is no thing at point that could be visited".into()
            } else {
                "No supported file location here".into()
            });
        }
        data.visit(&session.visits, point, thing)
    }

    pub(crate) fn status_visit_worktree(&self) -> Result<StatusAction, String> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref().ok_or("Not a status buffer")?;
        let data = session.data.as_ref().ok_or("Status is not ready")?;
        let point = self.point();
        if doc
            .sections
            .as_ref()
            .is_some_and(|sections| sections.collapse_point(point) != point)
        {
            return Err("No file at point".into());
        }
        data.visit_worktree(&session.visits, point)
    }

    pub(crate) fn status_preview_target(&self) -> Option<StatusPreviewTarget> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref()?;
        let point = self.point();
        if doc
            .sections
            .as_ref()
            .is_some_and(|sections| sections.collapse_point(point) != point)
        {
            return None;
        }
        Status::preview_target_at(&session.visits, point)
    }

    pub(crate) fn status_copy_value(&self) -> Option<String> {
        let doc = self.document.borrow();
        let session = doc.status.as_ref()?;
        let sections = doc.sections.as_ref()?;
        let point = sections.collapse_point(self.point());
        let lookup = |pos: crate::text::ByteOffset| -> Option<String> {
            let idx = session
                .copy_spans
                .partition_point(|(range, _)| range.end <= pos);
            session
                .copy_spans
                .get(idx)
                .filter(|(range, _)| range.contains(&pos))
                .map(|(_, value)| value.clone())
        };
        if let Some(value) = lookup(point) {
            return Some(value);
        }
        let mut idx = sections.current(point);
        while idx != 0 {
            if let Some(value) = lookup(sections.nodes()[idx].range.start) {
                return Some(value);
            }
            idx = sections.nodes()[idx].parent?;
        }
        None
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

    pub(crate) fn status_group_at_point(&self) -> Option<crate::status::Group> {
        let doc = self.document.borrow();
        doc.status.as_ref()?;
        let sections = doc.sections.as_ref()?;
        let nodes = sections.nodes();
        let mut index = sections.current(self.point());
        while let Some(parent) = nodes[index].parent {
            if parent == 0 {
                return match nodes[index].key.as_str() {
                    "untracked" => Some(crate::status::Group::Untracked),
                    "unstaged" => Some(crate::status::Group::Unstaged),
                    "staged" => Some(crate::status::Group::Staged),
                    "unmerged" => Some(crate::status::Group::Unmerged),
                    _ => None,
                };
            }
            index = parent;
        }
        None
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
        if let Some(session) = &mut doc.diff
            && !matches!(session.comparison.kind, crate::diff::DiffKind::Range { .. })
        {
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
