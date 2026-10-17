//! CPU summary, load rings and the per-core utilization matrix.
use super::gauges::ring;
use super::resources::{Tones, figure, heading, mono, panel};
use super::*;
use gpui_kit::component::tooltip::Tooltip;
use sailry_protocol::host::metrics::LoadAverage;

/// Cores up to this count render as rows of lit cells; more use one bar each.
const ROW_LIMIT: usize = 16;

impl Monitor {
    pub(super) fn cpu_card(&self, cx: &App) -> impl IntoElement {
        let tones = Tones::of(cx);
        let sample = self.view.sample.as_deref();
        let cores = sample.map_or(&[][..], |sample| sample.core_basis_points.as_slice());
        let usage = sample.and_then(|sample| sample.cpu_basis_points);
        // Load averages are only meaningful relative to a known core count.
        let load = sample
            .and_then(|sample| sample.load.as_ref())
            .filter(|_| !cores.is_empty());
        panel("host-cpu", cx).child(
            h_flex()
                .flex_wrap()
                .gap_x_7()
                .gap_y_5()
                .min_w_0()
                .debug_selector(|| "host-cpu".into())
                .child(
                    h_flex()
                        .flex_basis(px(300.))
                        .flex_shrink(1.)
                        .min_w_0()
                        .gap_4()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(heading("metrics_cpu", tones.cpu, cx))
                                .child(figure(
                                    usage.map_or_else(
                                        || "—".into(),
                                        |value| format!("{:.1}", value as f64 / 100.),
                                    ),
                                    if usage.is_some() { "%" } else { "" },
                                    cx,
                                ))
                                .child(
                                    h_flex()
                                        .gap_4()
                                        .whitespace_nowrap()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .debug_selector(|| "host-cpu-detail".into())
                                        .when(!cores.is_empty(), |row| {
                                            row.child(
                                                rust_i18n::t!("host_cores", count = cores.len())
                                                    .to_string(),
                                            )
                                        })
                                        .when_some(load, |row, load| {
                                            row.child(
                                                h_flex().gap_1().child(tr("host_load")).child(
                                                    mono(
                                                        div()
                                                            .text_color(cx.theme().foreground)
                                                            .child(
                                                                [load.one, load.five, load.fifteen]
                                                                    .map(|value| {
                                                                        format!(
                                                                            "{:.1}",
                                                                            value as f64 / 100.
                                                                        )
                                                                    })
                                                                    .join("  "),
                                                            ),
                                                        cx,
                                                    ),
                                                ),
                                            )
                                        }),
                                ),
                        )
                        .when_some(load, |row, load| row.child(rings(load, cores.len(), tones))),
                )
                .child(
                    div()
                        .flex_1()
                        .flex_basis(px(520.))
                        .min_w_0()
                        .pl_7()
                        .border_l_1()
                        .border_color(cx.theme().border)
                        .debug_selector(|| "host-cores".into())
                        .child(matrix(cores, tones, cx)),
                ),
        )
    }
}

/// Concentric 1, 5 and 15 minute load rings, each relative to the core count.
fn rings(load: &LoadAverage, cores: usize, tones: Tones) -> Div {
    div().relative().size(px(84.)).flex_shrink_0().children(
        [(load.one, 39.5), (load.five, 29.5), (load.fifteen, 19.5)]
            .into_iter()
            .enumerate()
            .map(|(index, (value, outer))| {
                let share = (value as f32 / 100. / cores as f32).min(1.);
                div().absolute().inset_0().child(ring(
                    ("host-load", index),
                    vec![(share, tones.load(tones.cpu, share * 100.))],
                    outer,
                    7.,
                    tones.track,
                ))
            }),
    )
}

fn matrix(cores: &[u32], tones: Tones, cx: &App) -> AnyElement {
    if cores.is_empty() {
        return div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(tr("host_sampling"))
            .into_any_element();
    }
    let count = cores.len();
    if count > ROW_LIMIT {
        let (columns, height) = match count {
            ..=32 => (16, 40.),
            ..=64 => (32, 32.),
            _ => (32, 22.),
        };
        return div()
            .grid()
            .grid_cols(columns)
            .gap_1()
            .children(cores.iter().enumerate().map(|(index, value)| {
                let percent = *value as f32 / 100.;
                let label = rust_i18n::t!(
                    "host_core",
                    index = index + 1,
                    value = format!("{percent:.0}%")
                )
                .to_string();
                div()
                    .id(("host-core", index))
                    .h(px(height))
                    .rounded(px(3.))
                    .bg(tones.track)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(
                        div()
                            .w_full()
                            .h(relative(percent / 100.))
                            .bg(tones.load(tones.cpu, percent)),
                    )
                    .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            }))
            .into_any_element();
    }
    let (columns, cells) = if count <= 4 { (1, 32) } else { (2, 20) };
    let (height, gap) = match count {
        ..=2 => (20., 8.),
        ..=4 => (14., 8.),
        ..=8 => (12., 10.),
        _ => (10., 7.),
    };
    div()
        .grid()
        .grid_cols(columns)
        .gap_x_7()
        .gap_y(px(gap))
        .children(cores.iter().enumerate().map(|(index, value)| {
            let percent = *value as f32 / 100.;
            let lit = (percent / 100. * cells as f32).round() as usize;
            let tone = tones.load(tones.cpu, percent);
            h_flex()
                .gap_2()
                .min_w_0()
                .child(mono(
                    div()
                        .w(px(18.))
                        .flex_shrink_0()
                        .text_right()
                        .text_size(px(10.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{:02}", index + 1)),
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .grid()
                        .grid_cols(cells as u16)
                        .gap(px(3.))
                        .children((0..cells).map(|cell| {
                            div().h(px(height)).rounded(px(2.)).bg(if cell < lit {
                                tone
                            } else {
                                tones.track
                            })
                        })),
                )
                .child(mono(
                    div()
                        .w(px(34.))
                        .flex_shrink_0()
                        .text_right()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{percent:.0}%")),
                    cx,
                ))
        }))
        .into_any_element()
}
