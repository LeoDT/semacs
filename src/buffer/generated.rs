use super::{Buffer, ByteOffset, Edit, EditSource};
use crate::section::Sections;
use crate::theme::StyleRole;
use std::ops::Range;
use std::sync::Arc;

impl Buffer {
    pub(super) fn install_projection(
        &mut self,
        text: &str,
        mut sections: Sections,
        styles: Vec<(Range<ByteOffset>, StyleRole)>,
    ) {
        let old = self.sections();
        if let Some(old) = &old {
            sections.restore(old);
        }
        let anchors: Vec<_> = self
            .document
            .borrow()
            .points
            .iter()
            .filter_map(|point| point.upgrade())
            .map(|point| {
                let anchor = old
                    .as_ref()
                    .map(|sections| sections.anchor(&self.text(), ByteOffset(point.get())));
                (point, anchor)
            })
            .collect();
        self.apply_validated_edit(Edit {
            snapshot: self.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(self.text().len()),
            text,
            source: EditSource::Command,
        });
        let mut doc = self.document.borrow_mut();
        for (point, anchor) in anchors {
            point.set(anchor.map_or(0, |anchor| sections.resolve(&doc.snapshot.text, &anchor).0));
        }
        doc.modified = false;
        doc.history = Default::default();
        doc.sections = Some(Arc::new(sections));
        doc.named_styles = styles.into();
    }

    pub(crate) fn toggle_section(&mut self) -> Result<(), String> {
        let point = self.point();
        let mut doc = self.document.borrow_mut();
        let sections = doc.sections.as_mut().ok_or("No sections in this buffer")?;
        let sections = Arc::make_mut(sections);
        sections.toggle(sections.current(point))?;
        let sections = doc.sections.as_ref().unwrap().clone();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(sections.collapse_point(ByteOffset(point.get())).0);
        }
        doc.touch();
        Ok(())
    }

    pub(crate) fn cycle_sections_global(&mut self) -> Result<(), String> {
        let mut doc = self.document.borrow_mut();
        let sections = doc.sections.as_mut().ok_or("No sections in this buffer")?;
        Arc::make_mut(sections).cycle_global();
        let sections = doc.sections.as_ref().unwrap().clone();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(sections.collapse_point(ByteOffset(point.get())).0);
        }
        doc.touch();
        Ok(())
    }

    pub(crate) fn reveal_section(&mut self, point: ByteOffset) {
        let mut doc = self.document.borrow_mut();
        if let Some(sections) = &mut doc.sections {
            Arc::make_mut(sections).reveal(point);
            doc.touch();
        }
    }
}
