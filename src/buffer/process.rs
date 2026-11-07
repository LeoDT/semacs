use super::Buffer;
use crate::git::Repository;
use crate::section::Sections;
use std::sync::Arc;

pub(super) struct ProcessSession {
    pub repository: Repository,
    operations: Vec<u64>,
}

impl Buffer {
    pub(crate) fn process(repository: Repository) -> Self {
        let buffer = Self::named(&format!(
            "*magit-process: {}*",
            repository.root.to_string_lossy().escape_debug()
        ));
        {
            let mut doc = buffer.document.borrow_mut();
            doc.mode = crate::mode::ModeId::MAGIT_PROCESS;
            doc.read_only = true;
            doc.process = Some(ProcessSession {
                repository,
                operations: Vec::new(),
            });
        }
        buffer
    }

    pub(crate) fn process_repository(&self) -> Option<Repository> {
        self.document
            .borrow()
            .process
            .as_ref()
            .map(|session| session.repository.clone())
    }

    pub(crate) fn install_process(&mut self, text: &str, sections: Sections, operations: Vec<u64>) {
        assert_eq!(sections.nodes().len(), operations.len() + 1);
        self.install_projection(text, sections, Vec::new());
        self.document
            .borrow_mut()
            .process
            .as_mut()
            .unwrap()
            .operations = operations;
    }

    pub(crate) fn process_at_point(&self) -> Option<u64> {
        let doc = self.document.borrow();
        let session = doc.process.as_ref()?;
        if self.point().0 == doc.snapshot.text.len() {
            return session.operations.last().copied();
        }
        let index = doc
            .sections
            .as_ref()?
            .current(self.point())
            .checked_sub(1)?;
        session.operations.get(index).copied()
    }

    pub(crate) fn detach_process(&mut self) {
        let mut doc = self.document.borrow_mut();
        if doc.process.take().is_some() {
            doc.sections = None;
            doc.named_styles = Arc::from([]);
            doc.mode = crate::mode::ModeId::FUNDAMENTAL;
            doc.read_only = false;
            doc.touch();
        }
    }
}
