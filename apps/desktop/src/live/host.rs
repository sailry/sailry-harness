//! Host overview composition, referenced from Code's Pi revision d9b56405.
use super::*;
use crate::activity;
use gpui_kit::component::{button::Button, list::ListItem, scroll::ScrollableElement, tag::Tag, *};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::activity::{Lane, lane};

mod disks;
mod monitor;
mod network;
mod process_actions;
mod processes;
mod resources;
#[cfg(test)]
mod tests;
mod transfers;
pub(crate) use monitor::Monitor;

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
                    && matches!(lane(session), Lane::Running | Lane::Waiting)
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
                    .max_w(px(crate::preview::CONTENT_WIDTH))
                    .p_6()
                    .flex_shrink_0()
                    .min_w_0()
                    .gap_6()
                    .child(
                        v_flex().gap_4().child(
                            h_flex()
                                .flex_wrap()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w(px(220.))
                                        .gap_3()
                                        .child(
                                            h_flex()
                                                .size_10()
                                                .flex_shrink_0()
                                                .justify_center()
                                                .rounded(cx.theme().radius)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .child(
                                                    host_icon(
                                                        info.and_then(|info| info.os.as_deref()),
                                                    )
                                                    .size_5(),
                                                ),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .gap_0()
                                                .line_height(relative(1.2))
                                                .child(
                                                    div()
                                                        .text_lg()
                                                        .font_semibold()
                                                        .truncate()
                                                        .debug_selector(|| "host-name".into())
                                                        .child(
                                                            info.and_then(|info| info.name.clone())
                                                                .map(SharedString::from)
                                                                .unwrap_or_else(|| {
                                                                    live.name(selected)
                                                                }),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .truncate()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(platform),
                                                ),
                                        ),
                                )
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
                                            .on_click(cx.listener(move |shell, _, window, cx| {
                                                shell.revoke_device(selected, window, cx)
                                            })),
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
                                                .child(
                                                    Tag::secondary()
                                                        .small()
                                                        .child(activity::status(session)),
                                                ),
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

pub(crate) fn host_icon(os: Option<&str>) -> Icon {
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
