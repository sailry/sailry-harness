use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{progress::Progress, scroll::ScrollableElement, *};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod data;
pub(crate) use data::Snapshot;
use data::{bytes, percent, rate, usage};

pub(crate) struct Metrics {
    pub host: usize,
    pub snapshot: Option<Snapshot>,
    pub error: Option<SharedString>,
}

impl Shell {
    pub(crate) fn sync_host_metrics(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.page != Page::Host || !self.layout.panel_open[Page::Host.panel_index()] {
            self.host_metrics = None;
        } else if self
            .host_metrics
            .as_ref()
            .is_none_or(|panel| panel.read(cx).host != self.host)
        {
            let host = self.host;
            self.host_metrics = Some(cx.new(|cx| {
                crate::feedback::observe_with(
                    window,
                    cx,
                    |view: &Metrics, _| {
                        view.error
                            .as_ref()
                            .map(|_| "metrics_error")
                            .into_iter()
                            .collect()
                    },
                    |view, key, _| {
                        crate::feedback::diagnostic(view.error.clone().unwrap_or_else(|| tr(key)))
                    },
                );
                Metrics {
                    host,
                    snapshot: Some(Snapshot::sample(host)),
                    error: None,
                }
            }));
        }
    }
}

impl Metrics {
    fn body(&self, cx: &App) -> AnyElement {
        let Some(snapshot) = &self.snapshot else {
            if self.error.is_some() {
                return div().into_any_element();
            }
            return v_flex()
                .gap_3()
                .debug_selector(|| "metrics-loading".into())
                .child(tr("metrics_loading"))
                .child(
                    Progress::new("metrics-loading")
                        .loading(true)
                        .xsmall()
                        .accessibility_label(tr("metrics_loading")),
                )
                .into_any_element();
        };
        v_flex()
            .gap_5()
            .debug_selector(|| "metrics-data".into())
            .child(
                v_flex()
                    .gap_2()
                    .child(fact("metrics_cpu", percent(snapshot.cpu)))
                    .child(fact(
                        "metrics_memory",
                        format!(
                            "{} / {}",
                            bytes(snapshot.memory_used as f64),
                            if snapshot.memory_total == 0 {
                                tr("host_unknown").to_string()
                            } else {
                                bytes(snapshot.memory_total as f64)
                            }
                        ),
                    ))
                    .when_some(
                        usage(snapshot.memory_used, snapshot.memory_total),
                        |this, value| {
                            this.child(
                                Progress::new("metrics-memory")
                                    .value(value)
                                    .xsmall()
                                    .accessibility_label(tr("metrics_memory")),
                            )
                        },
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_semibold().child(tr("metrics_network")))
                    .child(fact(
                        "metrics_download",
                        rate(snapshot.received, snapshot.interval),
                    ))
                    .child(fact(
                        "metrics_upload",
                        rate(snapshot.transmitted, snapshot.interval),
                    )),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(div().font_semibold().child(tr("metrics_disks")))
                    .when(snapshot.disks.is_empty(), |this| {
                        this.child(crate::empty_state::card(
                            IconName::HardDrive,
                            "metrics_no_disks",
                            cx,
                        ))
                    })
                    .children(snapshot.disks.iter().enumerate().map(|(index, disk)| {
                        v_flex()
                            .gap_1()
                            .min_w_0()
                            .debug_selector(move || format!("metrics-disk-{index}"))
                            .child(div().truncate().text_sm().child(disk.mount.clone()))
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} / {} / {}",
                                        disk.file_system,
                                        bytes(disk.total.saturating_sub(disk.available) as f64),
                                        bytes(disk.total as f64)
                                    )),
                            )
                            .child(fact("metrics_read", bytes(disk.read as f64)))
                            .child(fact("metrics_written", bytes(disk.written as f64)))
                    })),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(div().font_semibold().child(tr("metrics_processes")))
                    .when(snapshot.processes.is_empty(), |this| {
                        this.child(crate::empty_state::card(
                            IconName::Cpu,
                            "metrics_no_processes",
                            cx,
                        ))
                    })
                    .children(
                        snapshot
                            .processes
                            .iter()
                            .enumerate()
                            .map(|(index, process)| {
                                v_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .debug_selector(move || format!("metrics-process-{index}"))
                                    .child(div().truncate().text_sm().child(process.name.clone()))
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(percent(process.cpu))
                                            .child(bytes(process.memory as f64)),
                                    )
                            }),
                    ),
            )
            .into_any_element()
    }
}

fn fact(label: &'static str, value: String) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .text_sm()
        .child(div().flex_1().min_w_0().child(tr(label)))
        .child(div().truncate().child(value))
}

impl Render for Metrics {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .debug_selector(|| "host-metrics".into())
            .child(
                crate::header::Header::new("metrics-header", cx)
                    .bordered(false)
                    .child(Icon::new(IconName::ChartPie).size_4())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(tr("metrics_title")),
                    ),
            )
            .child(
                v_flex()
                    .id("metrics-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_4()
                    .gap_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("metrics_preview")),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .debug_selector(|| "metrics-host".into())
                            .child(tr(if self.host == 0 {
                                "local_host_name"
                            } else {
                                "remote_host_name"
                            })),
                    )
                    .child(self.body(cx)),
            )
    }
}
