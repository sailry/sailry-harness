//! Host overview composition, referenced from Code's Pi revision d9b56405.
use super::*;
use crate::activity;
use gpui_kit::component::{button::Button, list::ListItem, scroll::ScrollableElement, tag::Tag, *};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::activity::Lane;

mod cpu;
mod disks;
mod gauges;
mod monitor;
mod network;
mod process_actions;
mod processes;
mod resources;
mod storage;
mod trend;
pub(crate) use monitor::Monitor;

/// Matches the statistics page so wide host dashboards keep readable rows.
const WIDTH: f32 = 1040.;

impl Shell {
    pub(crate) fn live_host_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let selected = live.selected;
        let snapshot = live.view.snapshot.as_ref();
        let info = live.info.as_ref();
        let local = selected == live.services.local.target();
        let active: Vec<_> = snapshot
            .into_iter()
            .flat_map(|snapshot| &snapshot.sessions)
            .filter(|session| {
                session.delegation.is_none()
                    && matches!(
                        self.session_lane(selected, session),
                        Lane::Running | Lane::Waiting
                    )
            })
            .collect();
        let platform = info
            .map(|info| {
                [
                    info.os.as_deref(),
                    info.os_version.as_deref(),
                    info.architecture.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| tr("host_unknown").to_string());
        let processes = self.host_monitor.as_ref().and_then(|monitor| {
            monitor
                .read(cx)
                .view
                .sample
                .as_ref()
                .map(|sample| sample.process_count)
        });
        let facts = [
            (
                "host_uptime",
                crate::workspace::uptime(info.map(|info| info.host_uptime_secs)),
            ),
            (
                "host_node_uptime",
                crate::workspace::uptime(info.map(|info| info.node_uptime_secs)),
            ),
            (
                "metrics_processes",
                processes.map_or("—".into(), |count| SharedString::from(count.to_string())),
            ),
        ];
        v_flex()
            .id("live-overview")
            .debug_selector(|| "live-overview".into())
            .size_full()
            .overflow_y_scrollbar()
            .items_center()
            .child(
                v_flex()
                    .debug_selector(|| "host-content".into())
                    .w_full()
                    .max_w(px(WIDTH))
                    .p_6()
                    .flex_shrink_0()
                    .min_w_0()
                    .gap_3p5()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .justify_between()
                            .gap_x_6()
                            .gap_y_3()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(220.))
                                    .gap_3()
                                    .child(
                                        h_flex()
                                            .size(px(44.))
                                            .flex_shrink_0()
                                            .justify_center()
                                            .rounded_full()
                                            .bg(cx.theme().secondary)
                                            .child(
                                                platform_icon(
                                                    info.and_then(|info| info.os.as_deref()),
                                                )
                                                .size_5(),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_0p5()
                                            .child(
                                                h_flex()
                                                    .min_w_0()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .min_w_0()
                                                            .text_lg()
                                                            .font_semibold()
                                                            .truncate()
                                                            .debug_selector(|| "host-name".into())
                                                            .child(
                                                                info.and_then(|info| {
                                                                    info.name.clone()
                                                                })
                                                                .map(SharedString::from)
                                                                .unwrap_or_else(|| {
                                                                    live.name(selected)
                                                                }),
                                                            ),
                                                    )
                                                    .child(
                                                        Tag::success()
                                                            .small()
                                                            .rounded_full()
                                                            .flex_shrink_0()
                                                            .child(tr("live_connected")),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(platform),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_x_7()
                                    .gap_y_2()
                                    .debug_selector(|| "host-facts".into())
                                    .children(facts.into_iter().map(|(label, value)| {
                                        v_flex()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(tr(label)),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .whitespace_nowrap()
                                                    .font_family(
                                                        cx.theme().mono_font_family.clone(),
                                                    )
                                                    .child(value),
                                            )
                                    }))
                                    .when_some(self.ports.get(&selected), |row, ports| {
                                        row.child(ports.read(cx).header(ports.clone(), cx))
                                    })
                                    .when(!local, |row| {
                                        row.child(
                                            Button::new("live-revoke")
                                                .label(tr("live_revoke"))
                                                .small()
                                                .disabled(live.pending)
                                                .debug_selector(|| "live-revoke".into())
                                                .on_click(cx.listener(
                                                    move |shell, _, window, cx| {
                                                        shell.revoke_device(selected, window, cx)
                                                    },
                                                )),
                                        )
                                    }),
                            ),
                    )
                    .when_some(self.host_monitor.clone(), |body, monitor| {
                        body.child(monitor)
                    })
                    .when(!active.is_empty(), |body| {
                        body.child(
                            v_flex()
                                .gap_3()
                                .child(div().font_semibold().child(tr("live_host_active")))
                                .children(active.into_iter().map(|session| {
                                    let id = session.id;
                                    let location = snapshot
                                        .into_iter()
                                        .flat_map(|s| &s.projects)
                                        .find(|project| Some(project.id) == session.project)
                                        .map(|project| project.name.clone())
                                        .unwrap_or_default();
                                    let target = session.clone();
                                    ListItem::new(format!("host-session-{id}"))
                                        .debug_selector(move || format!("host-session-{id}"))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .gap_3()
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .child(activity::title(session)),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .truncate()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(location),
                                                        ),
                                                )
                                                .child(Tag::secondary().small().child(
                                                    activity::status_with_lane(
                                                        session,
                                                        self.session_lane(selected, session),
                                                    ),
                                                )),
                                        )
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            shell.reveal_session(target.clone(), window, cx)
                                        }))
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub(crate) fn platform_icon(os: Option<&str>) -> Icon {
    match os.unwrap_or_default().to_ascii_lowercase().as_str() {
        "darwin" | "macos" | "mac os" | "mac os x" => Icon::default().path("icons/os/apple.svg"),
        value if value.contains("windows") => Icon::default().path("icons/os/windows11.svg"),
        value
            if value.contains("linux")
                || [
                    "ubuntu", "debian", "fedora", "arch", "centos", "alpine", "opensuse",
                    "red hat", "pop!", "nixos", "mint",
                ]
                .iter()
                .any(|name| value.contains(name)) =>
        {
            Icon::default().path("icons/os/linux.svg")
        }
        _ => Icon::new(IconName::Cpu),
    }
}
