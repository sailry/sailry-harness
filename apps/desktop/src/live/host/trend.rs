//! One-minute CPU, GPU and memory utilization on a shared percentage axis.
use super::resources::{Tones, clock, legend, panel, pending, percent, title};
use super::*;
use gpui_kit::component::chart::AreaChart;

struct Reading {
    time: String,
    cpu: f64,
    gpu: Option<f64>,
    memory: Option<f64>,
}

impl Monitor {
    pub(super) fn trend_card(&self, cx: &App) -> impl IntoElement {
        let tones = Tones::of(cx);
        let sample = self.view.sample.as_deref();
        let memory = sample
            .and_then(|sample| sample.memory.as_ref())
            .filter(|memory| memory.total_bytes > 0);
        let gpu = sample.and_then(|sample| {
            sample
                .gpus
                .iter()
                .find(|gpu| Some(&gpu.id) == self.gpu.as_ref())
        });
        let mut points = Vec::new();
        for point in &self.view.history {
            let Some(cpu) = point.cpu_basis_points else {
                points.clear();
                continue;
            };
            points.push(Reading {
                time: clock(point.sampled_at_ms),
                cpu: cpu as f64 / 100.,
                gpu: self
                    .gpu
                    .as_ref()
                    .and_then(|id| point.gpus.get(id))
                    .copied()
                    .flatten()
                    .map(|value| value as f64 / 100.),
                memory: memory
                    .zip(point.memory_bytes)
                    .map(|(memory, used)| used as f64 * 100. / memory.total_bytes as f64),
            });
        }
        let with_gpu = points.iter().all(|point| point.gpu.is_some());
        let with_memory = points.iter().all(|point| point.memory.is_some());
        let body = div()
            .h(px(180.))
            .w_full()
            .min_w_0()
            .debug_selector(|| "host-trend-chart".into());
        let chart = if points.len() < 2 {
            body.child(pending(points.len(), cx))
        } else {
            let mut chart = AreaChart::new(points)
                .id("host-trend-chart")
                .x(|point| point.time.clone())
                .y(|point| point.cpu)
                .stroke(tones.cpu)
                .fill(tones.cpu.opacity(0.14))
                .name(tr("metrics_cpu"))
                .linear();
            if with_gpu {
                chart = chart
                    .y(|point| point.gpu.unwrap_or_default())
                    .stroke(tones.gpu)
                    .fill(tones.gpu.opacity(0.1))
                    .name(tr("metrics_gpu"))
                    .linear();
            }
            if with_memory {
                chart = chart
                    .y(|point| point.memory.unwrap_or_default())
                    .stroke(tones.memory)
                    .fill(gpui_kit::transparent_black())
                    .name(tr("metrics_memory"))
                    .linear();
            }
            body.child(
                chart
                    .y_domain(0., 100.)
                    .y_padding(0., 0.)
                    .y_axis(true)
                    .y_tick_count(5)
                    .y_tick_format(|value| format!("{value:.0}"))
                    .x_tick_count(5)
                    .grid_dashed(true)
                    .tooltip_title(|point| point.time.clone().into())
                    .tooltip_value(|_, _, value| format!("{value:.1}%").into()),
            )
        };
        let memory_percent = memory.map(|memory| {
            ((memory.total_bytes - memory.available_bytes) as f64 * 10000.
                / memory.total_bytes as f64)
                .round() as u32
        });
        panel("host-trend", cx)
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_between()
                    .gap_3()
                    .child(title("host_utilization"))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_4()
                            .child(legend(
                                "metrics_cpu",
                                percent(sample.and_then(|sample| sample.cpu_basis_points)),
                                tones.cpu,
                                cx,
                            ))
                            .when(gpu.is_some(), |row| {
                                row.child(legend(
                                    "metrics_gpu",
                                    percent(gpu.and_then(|gpu| gpu.usage_basis_points)),
                                    tones.gpu,
                                    cx,
                                ))
                            })
                            .child(legend(
                                "metrics_memory",
                                percent(memory_percent),
                                tones.memory,
                                cx,
                            )),
                    ),
            )
            .child(chart)
    }
}
