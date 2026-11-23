use super::{Buffer, ByteOffset, Edit, EditSource};
use crate::diff::{Comparison, Diff, DiffNavigation, RenderedDiff, Visit};
use std::sync::Arc;

pub(super) struct DiffSession {
    pub(super) comparison: Comparison,
    requested: Comparison,
    request: u64,
    data: Option<Arc<Diff>>,
    rendered: Option<DiffNavigation>,
    pub(super) stale: bool,
}

impl Buffer {
    pub(crate) fn diff(comparison: Comparison) -> Self {
        let mut buffer = Self::named(&format!(
            "*magit-diff {}{}: {}*",
            comparison.kind.name(),
            if comparison.environment == crate::diff::ReadEnvironment::Commit {
                " [commit]"
            } else {
                ""
            },
            comparison.repository.root.to_string_lossy().escape_debug()
        ));
        buffer.set_mode(crate::mode::ModeId::MAGIT_DIFF);
        {
            let mut doc = buffer.document.borrow_mut();
            doc.read_only = true;
            doc.diff = Some(DiffSession {
                requested: comparison.clone(),
                comparison,
                request: 0,
                data: None,
                rendered: None,
                stale: false,
            });
        }
        buffer
    }

    pub(crate) fn version_snapshot(name: &str, text: &str) -> Self {
        let mut buffer = Self::named(name);
        buffer.apply_validated_edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(0),
            text,
            source: EditSource::Command,
        });
        let mut doc = buffer.document.borrow_mut();
        doc.read_only = true;
        doc.modified = false;
        drop(doc);
        buffer
    }

    pub(crate) fn comparison(&self) -> Option<Comparison> {
        self.document
            .borrow()
            .diff
            .as_ref()
            .map(|session| session.comparison.clone())
    }

    pub(crate) fn begin_diff_request(&mut self) -> Result<(Comparison, u64), String> {
        let comparison = self.comparison().ok_or("Not a diff buffer")?;
        let request = self.begin_diff_request_for(comparison.clone())?;
        Ok((comparison, request))
    }

    pub(crate) fn begin_diff_request_for(&mut self, comparison: Comparison) -> Result<u64, String> {
        let mut doc = self.document.borrow_mut();
        let session = doc.diff.as_mut().ok_or("Not a diff buffer")?;
        if !session.comparison.same_buffer(&comparison) {
            return Err("Wrong diff target".into());
        }
        session.requested = comparison;
        session.request = session
            .request
            .checked_add(1)
            .expect("diff request identity exhausted");
        Ok(session.request)
    }

    pub(crate) fn diff_request_matches(&self, request: u64, comparison: &Comparison) -> bool {
        self.document
            .borrow()
            .diff
            .as_ref()
            .is_some_and(|session| session.request == request && &session.requested == comparison)
    }

    pub(crate) fn install_diff(
        &mut self,
        request: u64,
        data: Diff,
        rendered: RenderedDiff,
    ) -> Result<bool, String> {
        if !self.diff_request_matches(request, &data.comparison) {
            return Ok(false);
        }
        let same_comparison =
            self.document.borrow().diff.as_ref().is_some_and(|session| {
                session.data.is_some() && session.comparison == data.comparison
            });
        if same_comparison {
            self.install_projection(&rendered.text, rendered.sections, rendered.styles);
        } else {
            self.reset_projection(&rendered.text, rendered.sections, rendered.styles);
        }
        let mut doc = self.document.borrow_mut();
        let session = doc.diff.as_mut().unwrap();
        session.comparison = data.comparison.clone();
        session.data = Some(Arc::new(data));
        session.rendered = Some(rendered.navigation);
        session.stale = false;
        Ok(true)
    }

    pub(crate) fn diff_visit(&self) -> Result<Visit, String> {
        let doc = self.document.borrow();
        let session = doc.diff.as_ref().ok_or("Not a diff buffer")?;
        session.rendered.as_ref().ok_or("Diff is not ready")?.visit(
            session.data.as_ref().ok_or("Diff is not ready")?,
            self.point(),
        )
    }

    pub(crate) fn diff_peer(&self) -> Result<ByteOffset, String> {
        let doc = self.document.borrow();
        let rendered = doc
            .diff
            .as_ref()
            .and_then(|session| session.rendered.as_ref())
            .ok_or("Diff is not ready")?;
        let sections = doc.sections.as_ref().ok_or("No sections")?;
        let mut index = sections.current(self.point());
        loop {
            let node = &sections.nodes()[index];
            if let Some((left, right)) = rendered
                .peers
                .iter()
                .find(|(left, right)| *left == node.range.start || *right == node.range.start)
            {
                return Ok(if *left == node.range.start {
                    *right
                } else {
                    *left
                });
            }
            index = node.parent.ok_or("No diffstat entry here")?;
        }
    }
}
