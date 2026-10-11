//! Native draw and platform submission timings for one workload window.
use gpui_kit::{
    App, AppContext, Entity, Window, WindowId, profiler::FrameEvent, profiler::FrameTimingCollector,
};
use serde_json::{Value, json};
use std::time::Instant;

pub(super) struct Frames {
    // The Kit monitor holds its shared trace guard without rendering a HUD or
    // scheduling extra frames that would alter the measured workload.
    _monitor: Entity<gpui_fps::FpsMonitor>,
    collector: FrameTimingCollector,
    window: WindowId,
    draws: Vec<f64>,
    submissions: Vec<f64>,
    first_present: Option<Instant>,
    last_present: Option<Instant>,
}

impl Frames {
    pub(super) fn new(window: &Window, cx: &mut App) -> Self {
        let monitor = cx.new(|cx| gpui_fps::FpsMonitor::new(window, cx));
        Self {
            _monitor: monitor,
            collector: FrameTimingCollector::new(),
            window: window.window_handle().window_id(),
            draws: Vec::new(),
            submissions: Vec::new(),
            first_present: None,
            last_present: None,
        }
    }

    pub(super) fn collect(&mut self) {
        for event in self.collector.collect_unseen() {
            match event {
                FrameEvent::Draw(timing) if timing.window_id == self.window => {
                    self.draws
                        .push(timing.draw_duration().as_secs_f64() * 1000.);
                }
                FrameEvent::Present(timing) if timing.window_id == self.window => {
                    self.first_present.get_or_insert(timing.present_end);
                    self.last_present = Some(timing.present_end);
                    self.submissions
                        .push(timing.present_duration().as_secs_f64() * 1000.);
                }
                _ => {}
            }
        }
    }

    pub(super) fn finish(mut self) -> Result<Value, String> {
        self.collect();
        if self.draws.is_empty() || self.submissions.is_empty() {
            return Err("native workload recorded no drawn or presented frames".into());
        }
        let rate = self
            .first_present
            .zip(self.last_present)
            .and_then(|(first, last)| {
                let seconds = last.duration_since(first).as_secs_f64();
                (seconds > 0.).then(|| (self.submissions.len() - 1) as f64 / seconds)
            });
        Ok(json!({
            "drawn_frames": self.draws.len(),
            "presented_frames": self.submissions.len(),
            "observed_present_rate_hz": rate,
            "draw_ms": distribution(&mut self.draws),
            "present_submission_ms": distribution(&mut self.submissions),
        }))
    }
}

fn distribution(samples: &mut [f64]) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({
        "mean": samples.iter().sum::<f64>() / samples.len() as f64,
        "p95": samples[(samples.len() - 1) * 95 / 100],
        "max": samples.last(),
    })
}
