use super::*;

#[derive(Clone, PartialEq)]
pub(crate) struct InitialViewport {
    settings: LayoutSettings,
    size: Size<Pixels>,
    window_size: Size<Pixels>,
}

impl InitialViewport {
    pub(crate) fn matches_window(&self, window: &Window) -> bool {
        self.window_size == window.viewport_size()
    }
}

pub(crate) struct PreparedViewport {
    viewport: InitialViewport,
    metrics: FontMeasure,
    index: WrapIndex,
    rows: HashMap<usize, Arc<ShapedRow>>,
    remaining: Option<WrapJob>,
}

pub(crate) struct InitialPreparation {
    cancelled: Arc<AtomicBool>,
    task: Task<Option<PreparedViewport>>,
}

impl InitialPreparation {
    pub(crate) async fn ready(mut self) -> Option<PreparedViewport> {
        (&mut self.task).await
    }
}

impl Drop for InitialPreparation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl BufferView {
    pub(crate) fn initial_viewport(
        &self,
        bounds: Bounds<Pixels>,
        window: &Window,
    ) -> InitialViewport {
        let size = text_bounds(bounds).size;
        let mut settings = self.settings.clone();
        settings.wrap_width = size.width;
        InitialViewport {
            settings,
            size,
            window_size: window.viewport_size(),
        }
    }

    pub(crate) fn prepare_initial(
        &self,
        viewport: InitialViewport,
        cx: &App,
    ) -> InitialPreparation {
        let snapshot = self.display_snapshot();
        let system = cx.text_system().clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        #[cfg(feature = "native-profiler")]
        let trace = crate::preparation_trace::Context::worker("initial-worker");
        let task = cx.background_executor().spawn(async move {
            #[cfg(feature = "native-profiler")]
            let _recording = trace.map(crate::preparation_trace::Context::enter);
            #[cfg(feature = "native-profiler")]
            let _trace =
                crate::preparation_trace::Span::new("initial-worker", snapshot.text().len());
            if worker_cancel.load(Ordering::Relaxed) {
                return None;
            }
            let mut metrics = FontMeasure::new(system.clone(), viewport.settings.clone());
            let job = WrapJob::new(snapshot, viewport.settings.clone(), None, None);
            let count = visible_row_count(viewport.size.height, viewport.settings.line_height);
            let mut index = job.preview(&mut metrics, count + 1, PREVIEW_BYTES);
            if index.row_count() == 0 {
                index = job.finish(&mut metrics, &worker_cancel)?;
            }
            let shaper = WindowTextSystem::new(system);
            let mut rows = HashMap::new();
            for row in index.visible_rows(0..count) {
                if worker_cancel.load(Ordering::Relaxed) {
                    return None;
                }
                rows.insert(
                    row.index,
                    Arc::new(ShapedRow::shape(row, &index, &mut metrics, &shaper)),
                );
            }
            if worker_cancel.load(Ordering::Relaxed) {
                return None;
            }
            let remaining = (!index.is_complete()).then_some(job);
            Some(PreparedViewport {
                viewport,
                metrics,
                index,
                rows,
                remaining,
            })
        });
        InitialPreparation { cancelled, task }
    }

    pub(crate) fn install_initial(
        &mut self,
        prepared: PreparedViewport,
        viewport: &InitialViewport,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut settings = self.settings.clone();
        settings.wrap_width = viewport.size.width;
        if prepared.viewport != *viewport
            || viewport.settings != settings
            || prepared.index.key().display != self.display_snapshot().id()
        {
            cx.background_executor()
                .spawn(async move {
                    drop(prepared);
                })
                .detach();
            return false;
        }
        self.settings = settings;
        self.metrics = Some(prepared.metrics);
        self.index = Some(prepared.index);
        self.cache = prepared.rows;
        if let Some(job) = prepared.remaining {
            self.finish_index(job, cx);
        }
        true
    }
}

#[cfg(test)]
#[path = "initial_tests.rs"]
mod tests;
