//! GPU and memory gauges built on Kit's donut-capable pie chart.
use super::resources::{Tones, bytes, figure, half, heading, legend, mono, panel, scaled};
use super::*;
use gpui_kit::component::{
    button::ButtonVariants,
    chart::PieChart,
    menu::{DropdownMenu, PopupMenuItem},
};

/// A non-interactive ring whose segments are shares of one full turn; the
/// remainder paints as the track.
pub(super) fn ring(
    id: impl Into<ElementId>,
    segments: Vec<(f32, Hsla)>,
    outer: f32,
    width: f32,
    track: Hsla,
) -> PieChart<(f32, Hsla)> {
    let mut used = 0.;
    let mut slices: Vec<_> = segments
        .into_iter()
        .map(|(share, tone)| {
            let share = share.clamp(0., 1. - used);
            used += share;
            (share, tone)
        })
        .collect();
    slices.push((1. - used, track));
    PieChart::new(slices)
        .id(id)
        .interactive(false)
        .value(|slice| slice.0)
        .color(|slice| slice.1)
        .outer_radius(outer)
        .inner_radius(outer - width)
}

/// An 84px gauge ring with its value centered.
fn dial(ring: PieChart<(f32, Hsla)>, center: String, cx: &App) -> Div {
    div()
        .relative()
        .size(px(84.))
        .flex_shrink_0()
        .child(div().absolute().inset_0().child(ring))
        .child(mono(
            h_flex()
                .absolute()
                .inset_0()
                .justify_center()
                .text_sm()
                .font_semibold()
                .child(center),
            cx,
        ))
}

fn summary(label: &'static str, tone: Hsla, value: String, unit: &str, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(heading(label, tone, cx))
        .child(figure(value, unit, cx))
}

fn strip(cx: &App) -> Div {
    div().pt_3().border_t_1().border_color(cx.theme().border)
}

impl Monitor {
    pub(super) fn gpu_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tones = Tones::of(cx);
        let sample = self.view.sample.as_deref();
        let gpus = sample.map(|sample| sample.gpus.clone()).unwrap_or_default();
        let gpu = gpus.iter().find(|gpu| Some(&gpu.id) == self.gpu.as_ref());
        let usage = gpu
            .and_then(|gpu| gpu.usage_basis_points)
            .map(|value| value as f32 / 100.);
        let name = gpu
            .map(|gpu| gpu.name.clone())
            .unwrap_or_else(|| tr("host_metric_unavailable").to_string());
        let detail = if gpus.len() > 1 {
            let owner = cx.entity();
            let selected = self.gpu.clone();
            let choices = gpus.clone();
            Button::new("host-gpu-select")
                .small()
                .ghost()
                .label(name)
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    choices.iter().fold(menu, |menu, gpu| {
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
                })
                .into_any_element()
        } else {
            div()
                .text_xs()
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(name)
                .into_any_element()
        };
        let history: Vec<_> = self
            .view
            .history
            .iter()
            .map(|point| {
                self.gpu
                    .as_ref()
                    .and_then(|id| point.gpus.get(id))
                    .copied()
                    .flatten()
                    .map_or(0., |value| value as f32 / 10000.)
            })
            .collect();
        let last = history.len().saturating_sub(1);
        half(panel("host-gpu", cx))
            .child(
                h_flex()
                    .gap_4()
                    .min_w_0()
                    .debug_selector(|| "host-gpu".into())
                    .child(
                        summary(
                            "metrics_gpu",
                            tones.gpu,
                            usage.map_or_else(|| "—".into(), |value| format!("{value:.1}")),
                            if usage.is_some() { "%" } else { "" },
                            cx,
                        )
                        .child(
                            h_flex()
                                .h_6()
                                .min_w_0()
                                .overflow_hidden()
                                .debug_selector(|| "host-gpu-detail".into())
                                .child(detail),
                        ),
                    )
                    .child(dial(
                        ring(
                            "host-gpu-ring",
                            vec![(usage.unwrap_or_default() / 100., tones.gpu)],
                            42.,
                            9.,
                            tones.track,
                        ),
                        usage.map_or_else(|| "—".into(), |value| format!("{value:.0}%")),
                        cx,
                    )),
            )
            .when(!history.is_empty(), |card| {
                card.child(
                    strip(cx).child(
                        h_flex()
                            .h(px(22.))
                            .items_end()
                            .gap(px(2.))
                            .debug_selector(|| "host-gpu-history".into())
                            .children(history.into_iter().enumerate().map(|(index, share)| {
                                div()
                                    .flex_1()
                                    .min_h(px(1.))
                                    .h(relative(share))
                                    .rounded(px(2.))
                                    .bg(if index == last {
                                        tones.gpu
                                    } else {
                                        tones.gpu.opacity(0.45)
                                    })
                            })),
                    ),
                )
            })
    }

    pub(super) fn memory_card(&self, cx: &App) -> impl IntoElement {
        let tones = Tones::of(cx);
        let sample = self.view.sample.as_deref();
        let memory = sample
            .and_then(|sample| sample.memory.as_ref())
            .filter(|memory| memory.total_bytes > 0);
        let swap = sample.and_then(|sample| sample.swap.as_ref());
        let used = memory.map(|memory| memory.total_bytes - memory.available_bytes);
        let cached = memory.map(|memory| memory.available_bytes.saturating_sub(memory.free_bytes));
        let share = |value: Option<u64>| {
            memory.zip(value).map_or(0., |(memory, value)| {
                value as f32 / memory.total_bytes as f32
            })
        };
        let (value, unit) = used.map_or_else(|| ("—".into(), ""), scaled);
        let center = used.map_or_else(|| "—".into(), |_| format!("{:.0}%", share(used) * 100.));
        let split = used.zip(cached).map(|(used, cached)| {
            h_flex()
                .flex_wrap()
                .gap_x(px(18.))
                .gap_y_1p5()
                .child(legend("host_memory_used", bytes(used), tones.memory, cx))
                .child(legend("host_memory_cached", bytes(cached), tones.cache, cx))
                .when_some(swap, |row, swap| {
                    row.child(legend(
                        "host_memory_swap",
                        bytes(swap.used_bytes),
                        tones.swap,
                        cx,
                    ))
                })
        });
        half(panel("host-memory", cx))
            .child(
                h_flex()
                    .gap_4()
                    .min_w_0()
                    .debug_selector(|| "host-memory".into())
                    .child(
                        summary("metrics_memory", tones.memory, value, unit, cx).child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .debug_selector(|| "host-memory-detail".into())
                                .child(memory.map_or_else(
                                    || "—".into(),
                                    |memory| {
                                        rust_i18n::t!(
                                            "host_memory_total",
                                            value = bytes(memory.total_bytes)
                                        )
                                        .to_string()
                                    },
                                )),
                        ),
                    )
                    .child(dial(
                        ring(
                            "host-memory-ring",
                            vec![(share(used), tones.memory), (share(cached), tones.cache)],
                            42.,
                            9.,
                            tones.track,
                        ),
                        center,
                        cx,
                    )),
            )
            .when_some(split, |card, split| card.child(strip(cx).child(split)))
    }
}
