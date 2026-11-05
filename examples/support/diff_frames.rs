use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use gpui_kit::profiler::journal::PresentedFrame;

use super::workload::{Scene, ms};

#[derive(Clone, Copy)]
pub struct Scope {
    pub step: i32,
    pub kind: &'static str,
    pub phase: &'static str,
    pub action: bool,
    pub start: Option<Instant>,
}

pub struct FrameLog {
    output: File,
    origin: Instant,
    count: usize,
}

impl FrameLog {
    pub fn new(report: &Path, origin: Instant) -> Self {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report.with_extension("frames.csv"))
            .unwrap();
        writeln!(output, "# CPU draw/submit only; no GPU execution or screen presentation; setup is not action latency").unwrap();
        writeln!(output, "frame,step,kind,phase,stage,action_start_ms,draw_start_ms,draw_end_ms,draw_cpu_ms,request_layout_ms,prepaint_ms,paint_ms,draw_other_ms,submit_cpu_ms,submit_end_ms,views,selected_view,min_visible_rows,fully_indexed_views,bytes,width_px,height_px,target_display,compatible_views,selected_compatible,selected_ready,shared_snapshot,target_kind,projection_rows,collapsed,point,scroll_y,budget_violations").unwrap();
        output.flush().unwrap();
        Self {
            output,
            origin,
            count: 0,
        }
    }

    pub fn record(
        &mut self,
        scope: Scope,
        frame: PresentedFrame,
        scene: Option<&Scene>,
    ) -> Vec<String> {
        let timing = frame.frame;
        let phases = timing.phases;
        let draw_ms = ms(timing.draw_duration());
        let mut violations = Vec::new();
        if draw_ms > 16. {
            violations.push(format!("draw_cpu_ms={draw_ms:.6} >16"));
        }
        if scope.action && scope.phase == "idle" {
            violations.push("idle_draw".into());
        }
        let selected = scene.and_then(|s| s.bodies.iter().find(|b| b.view == s.selected));
        let target_kind = selected.map_or("other", |b| {
            if b.kind.starts_with("*magit-diff staged:") {
                "staged"
            } else if b.kind.starts_with("*magit-diff unstaged:") {
                "unstaged"
            } else {
                "other"
            }
        });
        writeln!(self.output, "{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{},{:.3},{:.3},\"{:?}\",{},{},{},{},{},{},{},{},{:.6},{}",
            self.count, scope.step, scope.kind, scope.phase, if scope.action { "action" } else { "setup" },
            scope.start.map_or_else(String::new, |start| format!("{:.6}", ms(start.duration_since(self.origin)))),
            ms(timing.draw_start.duration_since(self.origin)), ms(timing.draw_end.duration_since(self.origin)), draw_ms,
            ms(phases.request_layout), ms(phases.prepaint), ms(phases.paint),
            ms(timing.draw_duration().saturating_sub(phases.request_layout + phases.prepaint + phases.paint)),
            ms(frame.presentation.present_end.duration_since(frame.presentation.present_start)),
            ms(frame.presentation.present_end.duration_since(self.origin)),
            scene.map_or(0, |s| s.bodies.len()), scene.map_or_else(String::new, |s| s.selected.to_string()),
            scene.map_or(0, |s| s.bodies.iter().map(|b| b.visible).min().unwrap_or(0)),
            scene.map_or(0, |s| s.bodies.iter().filter(|b| b.exact && b.ready).count()),
            selected.map_or(0, |b| b.bytes), selected.map_or(0., |b| b.width), selected.map_or(0., |b| b.height),
            selected.map(|b| b.display),
            scene.map_or(0, |s| s.bodies.iter().filter(|b| b.exact).count()),
            selected.is_some_and(|b| b.exact), selected.is_some_and(|b| b.ready),
            selected.is_some_and(|selected| scene.unwrap().bodies.iter().all(|b| b.snapshot == selected.snapshot)),
            target_kind, selected.map_or(0, |b| b.rows), selected.is_some_and(|b| b.collapsed),
            selected.map_or(0, |b| b.point), selected.map_or(0., |b| b.scroll), violations.join(";")).unwrap();
        self.count += 1;
        self.output.flush().unwrap();
        violations
    }

    pub fn finish(&mut self, success: bool) {
        writeln!(self.output, "# completed={success}").unwrap();
        self.output.flush().unwrap();
    }
}
