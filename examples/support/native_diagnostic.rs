use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use gpui_kit::profiler::ViewTiming;
use gpui_kit::profiler::journal::{
    ForegroundEvent, ForegroundJournalCollector, ForegroundJournalEntry, PresentedFrame,
};
use semacs::preparation_trace::{Capture, Recording};

use crate::{EditStamp, PaintState, millis};

struct Frame {
    step: usize,
    phase: &'static str,
    action_start: Option<Instant>,
    target: Option<EditStamp>,
    timing: PresentedFrame,
    paint: Option<PaintState>,
}

pub struct Trace {
    collector: ForegroundJournalCollector,
    origin: Instant,
    frames: Vec<Frame>,
    views: Vec<ViewTiming>,
    lost: u64,
    frame_output: File,
    view_output: File,
    preparation_output: File,
    preparation: Capture,
    recording: Option<Recording>,
}

fn create(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("new diagnostic sidecar; refusing to overwrite")
}

fn offset(at: Instant, origin: Instant) -> f64 {
    if at >= origin {
        millis(at - origin)
    } else {
        -millis(origin - at)
    }
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.6}"))
}

impl Trace {
    pub fn new(report: &Path, collector: ForegroundJournalCollector) -> Self {
        let preparation = Capture::default();
        let recording = Some(preparation.enter());
        Self {
            collector,
            origin: Instant::now(),
            frames: Vec::new(),
            views: Vec::new(),
            lost: 0,
            frame_output: create(&report.with_extension("frames.csv")),
            view_output: create(&report.with_extension("views.csv")),
            preparation_output: create(&report.with_extension("preparation.csv")),
            preparation,
            recording,
        }
    }

    pub fn poll(&mut self) -> Result<(), String> {
        let drained = self.collector.collect_unseen();
        self.lost += drained.lost;
        for entry in drained.entries {
            if let ForegroundJournalEntry::Event(ForegroundEvent::View(view)) = entry {
                self.views.push(view);
            }
        }
        if self.lost != 0 {
            return Err("diagnostic journal overflow invalidates trace".into());
        }
        Ok(())
    }

    pub fn record(
        &mut self,
        step: usize,
        phase: &'static str,
        action_start: Option<Instant>,
        target: Option<EditStamp>,
        timing: PresentedFrame,
        paint: Option<PaintState>,
    ) {
        self.frames.push(Frame {
            step,
            phase,
            action_start,
            target,
            timing,
            paint,
        });
    }

    pub fn finish(&mut self, success: bool) {
        let (samples, lost, stopped) = self.preparation.finish();
        self.recording.take();
        writeln!(self.frame_output, "# preparation_trace_version=1").unwrap();
        writeln!(self.preparation_output, "# nested wall-clock spans overlap; foreground detail only; unfinished worker end is observation cutoff, NOT completion; no raw text recorded").unwrap();
        writeln!(
            self.preparation_output,
            "id,parent,frame,lane,phase,bytes,start_ms,end_ms,complete,total_ms"
        )
        .unwrap();
        for (id, sample) in samples.iter().enumerate() {
            let end = sample.end.unwrap_or(stopped);
            let frame = (sample.lane == "foreground")
                .then(|| {
                    self.frames.iter().position(|frame| {
                        sample.start >= frame.timing.frame.draw_start
                            && end <= frame.timing.frame.draw_end
                    })
                })
                .flatten();
            writeln!(
                self.preparation_output,
                "{id},{},{},{},{},{},{:.6},{:.6},{},{:.6}",
                sample.parent.map_or_else(String::new, |id| id.to_string()),
                frame.map_or_else(String::new, |id| id.to_string()),
                sample.lane,
                sample.phase,
                sample.bytes,
                offset(sample.start, self.origin),
                offset(end, self.origin),
                sample.end.is_some(),
                millis(end - sample.start),
            )
            .unwrap();
        }
        writeln!(
            self.preparation_output,
            "# completed_open={success}; lost_entries={lost}; acceptance=false"
        )
        .unwrap();
        self.preparation_output.flush().unwrap();
        writeln!(self.frame_output, "# diagnostic_only=true; acceptance=false; GPU execution / compositor presentation unmeasured").unwrap();
        writeln!(self.frame_output, "# buffered in memory until stop; setup frames are not action latency; draw_other is draw minus the three measured phases, NOT a GPU metric").unwrap();
        writeln!(self.frame_output, "frame,step,phase,stage,draw_start_ms,draw_end_ms,action_to_draw_ms,draw_cpu_ms,request_layout_ms,prepaint_ms,paint_ms,draw_other_ms,submit_cpu_ms,draw_to_submit_gap_ms,action_to_submit_ms,invalidations,target_match,bytes,indexed_bytes,visible_rows,scanned_bytes,layout_ready,compatible,marked,width_px,height_px").unwrap();
        writeln!(self.view_output, "# profiler retains view self_duration >= 0.5 ms only; nested total times overlap; missing views do not mean zero cost").unwrap();
        writeln!(
            self.view_output,
            "frame,step,phase,view,view_phase,start_ms,end_ms,total_ms,self_ms"
        )
        .unwrap();
        for (index, frame) in self.frames.iter().enumerate() {
            let timing = frame.timing.frame;
            let phases = timing.phases;
            let stage = if frame
                .action_start
                .is_some_and(|start| timing.draw_start >= start)
            {
                "measured"
            } else {
                "setup"
            };
            let paint = frame.paint;
            let optional = |field: fn(PaintState) -> usize| {
                paint.map_or_else(String::new, |p| field(p).to_string())
            };
            let flag = |field: fn(PaintState) -> bool| {
                paint.map_or_else(String::new, |p| field(p).to_string())
            };
            let target = if stage == "measured" {
                frame
                    .target
                    .zip(paint)
                    .map_or_else(String::new, |(target, paint)| {
                        (target == paint.stamp).to_string()
                    })
            } else {
                String::new()
            };
            writeln!(self.frame_output, "{index},{},{},{stage},{:.6},{:.6},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{target},{},{},{},{},{},{},{},{},{}",
                frame.step, frame.phase,
                offset(timing.draw_start, self.origin), offset(timing.draw_end, self.origin),
                cell(frame.action_start.map(|start| offset(timing.draw_start, start))),
                millis(timing.draw_duration()), millis(phases.request_layout), millis(phases.prepaint), millis(phases.paint),
                millis(timing.draw_duration().saturating_sub(phases.request_layout + phases.prepaint + phases.paint)),
                millis(frame.timing.presentation.present_duration()),
                millis(frame.timing.presentation.present_start.saturating_duration_since(timing.draw_end)),
                cell(frame.action_start.map(|start| offset(frame.timing.presentation.present_end, start))),
                timing.invalidations,
                optional(|p| p.bytes), optional(|p| p.indexed), optional(|p| p.visible), optional(|p| p.scanned),
                flag(|p| p.ready), flag(|p| p.compatible), flag(|p| p.marked),
                paint.map_or_else(String::new, |p| format!("{:.1}", p.width)),
                paint.map_or_else(String::new, |p| format!("{:.1}", p.height)),
            ).unwrap();
            for view in self
                .views
                .iter()
                .filter(|view| view.start >= timing.draw_start && view.end <= timing.draw_end)
            {
                writeln!(
                    self.view_output,
                    "{index},{},{},\"{}\",{},{:.6},{:.6},{:.6},{:.6}",
                    frame.step,
                    frame.phase,
                    view.name.replace('"', "\"\""),
                    view.phase.as_str(),
                    offset(view.start, self.origin),
                    offset(view.end, self.origin),
                    millis(view.end - view.start),
                    millis(view.self_duration),
                )
                .unwrap();
            }
        }
        for output in [&mut self.frame_output, &mut self.view_output] {
            writeln!(
                output,
                "# completed_open={success}; lost_entries={}; acceptance=false",
                self.lost
            )
            .unwrap();
            output.flush().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{frame, paint};
    use std::time::Duration;

    #[test]
    fn trace_keeps_slow_frame_phases_target_identity_and_missing_observations() {
        use gpui_kit::profiler::{DrawPhases, ViewPhase};
        let cx = gpui_kit::TestAppContext::single();
        let collector = cx.update(|cx| cx.foreground_journal().collector());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".pi/tmp");
        std::fs::create_dir_all(&root).unwrap();
        let report = root.join(format!(
            "m2-trace-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut trace = Trace::new(&report, collector);
        let start = trace.origin;
        let mut slow = frame(start);
        slow.frame.draw_end = start + Duration::from_millis(42);
        slow.frame.phases = DrawPhases {
            request_layout: Duration::from_millis(1),
            prepaint: Duration::from_millis(30),
            paint: Duration::from_millis(9),
        };
        slow.presentation.present_start = start + Duration::from_millis(43);
        slow.presentation.present_end = start + Duration::from_millis(45);
        {
            let _span = semacs::preparation_trace::Span::new("prepare", 123);
        }
        let body = paint(start);
        trace.record(1, "open", Some(start), Some(body.stamp), slow, Some(body));
        trace.record(1, "open", None, None, frame(start), None);
        trace.views.push(ViewTiming {
            name: "view,\"name",
            phase: ViewPhase::Prepaint,
            start,
            end: start + Duration::from_millis(30),
            self_duration: Duration::from_millis(29),
        });
        trace.finish(false);
        let frames = std::fs::read_to_string(report.with_extension("frames.csv")).unwrap();
        let row: Vec<_> = frames
            .lines()
            .find(|line| line.starts_with("0,"))
            .unwrap()
            .split(',')
            .collect();
        assert_eq!(row.len(), 26);
        assert_eq!(
            &row[7..15],
            [
                "42.000000",
                "1.000000",
                "30.000000",
                "9.000000",
                "2.000000",
                "2.000000",
                "1.000000",
                "45.000000"
            ]
        );
        assert_eq!(row[16], "true");
        assert!(frames.contains("1,1,open,setup,"));
        assert!(frames.contains("completed_open=false; lost_entries=0; acceptance=false"));
        let views = std::fs::read_to_string(report.with_extension("views.csv")).unwrap();
        assert!(views.contains("\"view,\"\"name\""));
        assert!(views.contains("29.000000"));
        let preparation =
            std::fs::read_to_string(report.with_extension("preparation.csv")).unwrap();
        assert!(preparation.contains(",foreground,prepare,123,"));
        assert!(preparation.contains("completed_open=false; lost_entries=0; acceptance=false"));
        drop(trace);
        std::fs::remove_file(report.with_extension("frames.csv")).unwrap();
        std::fs::remove_file(report.with_extension("views.csv")).unwrap();
        std::fs::remove_file(report.with_extension("preparation.csv")).unwrap();
    }
}
