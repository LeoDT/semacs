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
        self.replace_projection(text, sections, styles);
    }

    pub(super) fn reset_projection(
        &mut self,
        text: &str,
        sections: Sections,
        styles: Vec<(Range<ByteOffset>, StyleRole)>,
    ) {
        self.apply_validated_edit(Edit {
            snapshot: self.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(self.text().len()),
            text,
            source: EditSource::Command,
        });
        let mut doc = self.document.borrow_mut();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(0);
        }
        doc.modified = false;
        doc.history = Default::default();
        doc.sections = Some(Arc::new(sections));
        doc.named_styles = styles.into();
    }

    pub(super) fn replace_projection(
        &mut self,
        text: &str,
        sections: Sections,
        styles: Vec<(Range<ByteOffset>, StyleRole)>,
    ) {
        let old = self.sections();
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

    pub(crate) fn cycle_section(&mut self) -> Result<(), String> {
        let point = self.point();
        let mut doc = self.document.borrow_mut();
        let sections = doc.sections.as_mut().ok_or("No sections in this buffer")?;
        Arc::make_mut(sections).cycle(point);
        let sections = doc.sections.as_ref().unwrap().clone();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(sections.collapse_point(ByteOffset(point.get())).0);
        }
        doc.touch();
        Ok(())
    }

    pub(crate) fn cycle_sections_diffs(&mut self, status_mode: bool) -> Result<(), String> {
        let mut doc = self.document.borrow_mut();
        let sections = doc.sections.as_mut().ok_or("No sections in this buffer")?;
        Arc::make_mut(sections).cycle_diffs(status_mode);
        let sections = doc.sections.as_ref().unwrap().clone();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(sections.collapse_point(ByteOffset(point.get())).0);
        }
        doc.touch();
        Ok(())
    }

    pub(crate) fn show_section_level(&mut self, level: usize, all: bool) -> Result<ByteOffset, String> {
        let point = self.point();
        let mut doc = self.document.borrow_mut();
        let sections = doc.sections.as_mut().ok_or("No sections in this buffer")?;
        let target = if all {
            Arc::make_mut(sections).show_level_all(point, level)
        } else {
            Arc::make_mut(sections).show_level(point, level)
        };
        let sections = doc.sections.as_ref().unwrap().clone();
        for point in doc.points.iter().filter_map(|point| point.upgrade()) {
            point.set(sections.collapse_point(ByteOffset(point.get())).0);
        }
        doc.touch();
        Ok(target)
    }

    pub(crate) fn reveal_section(&mut self, point: ByteOffset) {
        let mut doc = self.document.borrow_mut();
        if let Some(sections) = &mut doc.sections {
            Arc::make_mut(sections).reveal(point);
            doc.touch();
        }
    }

    pub(crate) fn is_range_hidden(&self, range: Range<ByteOffset>) -> bool {
        self.document
            .borrow()
            .sections
            .as_ref()
            .is_some_and(|sections| sections.is_range_hidden(range))
    }

    pub(crate) fn isearch_temp_open(
        &mut self,
        range: Range<ByteOffset>,
        opened: &mut Vec<String>,
    ) -> bool {
        let mut doc = self.document.borrow_mut();
        let Some(sections) = &mut doc.sections else {
            return false;
        };
        let changed = Arc::make_mut(sections).temp_open_for_range(range, opened);
        if changed {
            let sections = doc.sections.as_ref().unwrap().clone();
            for point in doc.points.iter().filter_map(|point| point.upgrade()) {
                if !std::rc::Rc::ptr_eq(&point, &self.point) {
                    point.set(sections.collapse_point(ByteOffset(point.get())).0);
                }
            }
            doc.touch();
        }
        changed
    }

    pub(crate) fn isearch_finalize_opened(
        &mut self,
        keep_range: Option<Range<ByteOffset>>,
        opened: &mut Vec<String>,
    ) -> bool {
        let mut doc = self.document.borrow_mut();
        let Some(sections) = &mut doc.sections else {
            opened.clear();
            return false;
        };
        let changed = Arc::make_mut(sections).finalize_temp_opened(keep_range, opened);
        if changed {
            let sections = doc.sections.as_ref().unwrap().clone();
            for point in doc.points.iter().filter_map(|point| point.upgrade()) {
                point.set(sections.collapse_point(ByteOffset(point.get())).0);
            }
            doc.touch();
        }
        changed
    }

    #[cfg(test)]
    pub(crate) fn install_test_sections(&mut self, sections: Sections) {
        let mut doc = self.document.borrow_mut();
        doc.sections = Some(Arc::new(sections));
        doc.touch();
    }
}
