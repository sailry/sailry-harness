//! Host-only composition of Kit charts and metric cards.
use super::{Monitor, *};
use gpui_kit::component::{
    button::ButtonVariants,
    chart::AreaChart,
    group_box::{GroupBox, GroupBoxVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

impl Monitor {
    pub(super) fn charts(&self, _: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sample = self.view.sample.as_deref();
        let gpu = sample.and_then(|sample| {
            sample
                .gpus
                .iter()
                .find(|gpu| Some(&gpu.id) == self.gpu.as_ref())
        });
        let memory = sample.and_then(|sample| sample.memory.as_ref());
        let cpu_points = series(self.view.history.iter().map(|point| {
            (
                point.sampled_at_ms,
                point
                    .cpu_basis_points
                    .map(|value| (value as f64 / 100., 0.)),
            )
        }));
        let gpu_points = series(self.view.history.iter().map(|point| {
            (
                point.sampled_at_ms,
                self.gpu
                    .as_ref()
                    .and_then(|id| point.gpus.get(id))
                    .copied()
                    .flatten()
                    .map(|value| (value as f64 / 100., 0.)),
            )
        }));
        let memory_points = series(self.view.history.iter().map(|point| {
            (
                point.sampled_at_ms,
                point
                    .memory_bytes
                    .map(|value| (value as f64 / 1_073_741_824., 0.)),
            )
        }));
        let owner = cx.entity();
        let gpus = sample.map(|sample| sample.gpus.clone()).unwrap_or_default();
        let selected = self.gpu.clone();
        let gpu_label = gpu
            .map(|gpu| gpu.name.clone())
            .unwrap_or_else(|| tr("host_metric_unavailable").to_string());
        let gpu_menu = Button::new("host-gpu-select")
            .small()
            .ghost()
            .label(gpu_label)
            .dropdown_caret(gpus.len() > 1)
            .disabled(gpus.len() < 2)
            .dropdown_menu(move |menu, _, _| {
                gpus.iter().fold(menu, |menu, gpu| {
                    let owner = owner.clone();
                    let id = gpu.id.clone();
                    menu.item(
                        PopupMenuItem::new(gpu.name.clone())
                            .checked(selected.as_ref() == Some(&gpu.id))
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |this, cx| {
                                    this.gpu = Some(id.clone());
                                    cx.notify();
                                })
                            }),
                    )
                })
            });
        v_flex()
            .gap_3()
            .w_full()
            .min_w_0()
            .debug_selector(|| "host-charts".into())
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_3()
                    .w_full()
                    .min_w_0()
                    .child(card(
                        "host-cpu",
                        "metrics_cpu",
                        percent(sample.and_then(|sample| sample.cpu_basis_points)),
                        chart(
                            "host-cpu-chart",
                            cpu_points,
                            cx.theme().chart_1,
                            "host_percent",
                            None,
                            cx,
                        ),
                        h_flex()
                            .h_6()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("host_cpu_total")),
                        cx,
                    ))
                    .child(card(
                        "host-gpu",
                        "metrics_gpu",
                        percent(gpu.and_then(|gpu| gpu.usage_basis_points)),
                        chart(
                            "host-gpu-chart",
                            gpu_points,
                            cx.theme().chart_3,
                            "host_percent",
                            None,
                            cx,
                        ),
                        h_flex().h_6().min_w_0().overflow_hidden().map(|body| {
                            if sample.is_some_and(|sample| sample.gpus.len() > 1) {
                                body.child(gpu_menu)
                            } else {
                                body.text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(gpu.map(|gpu| gpu.name.clone()).unwrap_or_else(|| {
                                        tr("host_metric_unavailable").to_string()
                                    }))
                            }
                        }),
                        cx,
                    ))
                    .child(card(
                        "host-memory",
                        "metrics_memory",
                        memory
                            .map(|memory| bytes(memory.total_bytes - memory.available_bytes))
                            .unwrap_or_else(|| "—".into()),
                        chart(
                            "host-memory-chart",
                            memory_points,
                            cx.theme().chart_5,
                            "host_gib",
                            None,
                            cx,
                        ),
                        h_flex()
                            .h_6()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                memory
                                    .map(|memory| {
                                        rust_i18n::t!(
                                            "host_memory_total",
                                            value = bytes(memory.total_bytes)
                                        )
                                        .to_string()
                                    })
                                    .unwrap_or_else(|| "—".into()),
                            ),
                        cx,
                    )),
            )
            .child(self.network_card(cx))
            .child(self.disk_card(cx))
    }
}

pub(super) fn card(
    id: &'static str,
    label: &'static str,
    value: String,
    chart: impl IntoElement,
    detail: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    GroupBox::new()
        .id(id)
        .fill()
        .min_w_0()
        .content_style(
            StyleRefinement::default()
                .flex_1()
                .gap_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg),
        )
        .child(
            v_flex()
                .min_w_0()
                .gap_2()
                .debug_selector(move || id.into())
                .child(
                    h_flex()
                        .h_8()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(label)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_right()
                                .text_base()
                                .when(matches!(id, "host-disk" | "host-network"), |value| {
                                    value.text_sm()
                                })
                                .font_semibold()
                                .when(id != "host-disk", |value| value.truncate())
                                .child(value),
                        ),
                )
                .child(chart)
                .child(
                    v_flex()
                        .min_w_0()
                        .debug_selector(move || format!("{id}-detail"))
                        .child(detail),
                ),
        )
}

pub(super) fn chart(
    id: &'static str,
    points: Vec<(String, f64, f64)>,
    color: Hsla,
    label: &'static str,
    second: Option<(Hsla, &'static str)>,
    cx: &App,
) -> impl IntoElement {
    div()
        .h_16()
        .when(second.is_some(), |chart| chart.h_20())
        .w_full()
        .min_w_0()
        .debug_selector(move || id.into())
        .map(|body| {
            if points.len() < 2 {
                body.child(
                    h_flex()
                        .size_full()
                        .justify_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr(if points.is_empty() {
                            "host_metric_unavailable"
                        } else {
                            "host_sampling"
                        })),
                )
            } else {
                if let Some((second_color, second_label)) = second {
                    return body.child(super::transfers::chart(
                        id,
                        points,
                        (color, tr(label)),
                        (second_color, tr(second_label)),
                    ));
                }
                let chart = AreaChart::new(points)
                    .id(id)
                    .x(|point| point.0.clone())
                    .y(|point| point.1)
                    .tooltip_value(|_, _, value| rounded(value).to_string().into())
                    .stroke(color)
                    .fill(color.opacity(0.18))
                    .name(tr(label))
                    .linear()
                    .x_axis(false)
                    .grid(false);
                body.child(chart)
            }
        })
}

pub(super) fn series(
    points: impl Iterator<Item = (u64, Option<(f64, f64)>)>,
) -> Vec<(String, f64, f64)> {
    let mut values = Vec::new();
    for (time, point) in points {
        if let Some((value, second)) = point {
            let label = chrono::DateTime::from_timestamp_millis(time as i64)
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("%H:%M:%S")
                        .to_string()
                })
                .unwrap_or_default();
            values.push((label, value, second));
        } else {
            values.clear();
        }
    }
    values
}

pub(super) fn percent(value: Option<u32>) -> String {
    value
        .map(|value| format!("{:.1}%", value as f64 / 100.))
        .unwrap_or_else(|| "—".into())
}

pub(super) fn rounded(value: f64) -> f64 {
    (value * 100.).round() / 100.
}

pub(super) fn bytes(value: u64) -> String {
    let (scale, unit) = if value >= 1_073_741_824 {
        (1_073_741_824., "GiB")
    } else if value >= 1_048_576 {
        (1_048_576., "MiB")
    } else if value >= 1024 {
        (1024., "KiB")
    } else {
        (1., "B")
    };
    format!("{:.1} {unit}", value as f64 / scale)
}

pub(super) fn rate(value: u64) -> String {
    format!("{}/s", bytes(value))
}

#[cfg(test)]
mod tests {
    use super::rounded;
    use core::prelude::v1::test;

    #[test]
    fn rounds_tooltip_values() {
        for (value, expected) in [
            (0.0452880859375, "0.05"),
            (109.00603675842285, "109.01"),
            (18.2, "18.2"),
            (0., "0"),
        ] {
            assert_eq!(rounded(value).to_string(), expected);
        }
    }
}
