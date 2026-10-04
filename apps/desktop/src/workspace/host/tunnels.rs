use crate::{
    shell::Shell,
    tr,
    workspace::model::tunnels::{Tunnel, ports_label},
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod editor;

impl Shell {
    pub(crate) fn host_tunnels(&self, host: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.workspace.tunnels.get(&host) else {
            return div().into_any_element();
        };
        v_flex()
            .gap_2()
            .debug_selector(|| "host-tunnels".into())
            .child(
                h_flex()
                    .gap_2()
                    .child(div().font_semibold().child(tr("tunnels_title")))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if state.loaded {
                                state.entries.len().to_string()
                            } else {
                                tr("host_unknown").to_string()
                            }),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("tunnels_preview")),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .debug_selector(|| "tunnel-ports".into())
                            .child(if state.ports.is_empty() {
                                tr("tunnel_ports_empty").to_string()
                            } else {
                                rust_i18n::t!(
                                    "tunnel_ports_value",
                                    ports = ports_label(&state.ports)
                                )
                                .to_string()
                            }),
                    )
                    .when(state.editable, |row| {
                        row.child(
                            Button::new("tunnel-edit")
                                .ghost()
                                .small()
                                .icon(IconName::Settings2)
                                .label(tr("tunnel_edit"))
                                .disabled(!state.loaded || state.error.is_some())
                                .debug_selector(|| "tunnel-edit".into())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.tunnel_editor(host, window, cx)
                                })),
                        )
                    }),
            )
            .when(!state.editable, |body| {
                body.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .debug_selector(|| "tunnel-readonly".into())
                        .child(tr("tunnel_readonly")),
                )
            })
            .when(!state.loaded && state.error.is_none(), |body| {
                body.child(
                    div()
                        .text_sm()
                        .debug_selector(|| "tunnels-loading".into())
                        .child(tr("tunnels_loading")),
                )
            })
            .when(state.loaded && state.entries.is_empty(), |body| {
                body.child(
                    crate::empty_state::card(IconName::Network, "tunnels_empty", cx)
                        .debug_selector(|| "tunnels-empty".into()),
                )
            })
            .children(state.entries.iter().map(|(&id, tunnel)| {
                let opened = tunnel.clone();
                let closed = tunnel.clone();
                h_flex()
                    .gap_3()
                    .py_1()
                    .debug_selector(move || format!("tunnel-row-{id}"))
                    .child(Icon::new(IconName::Network).size_4().flex_shrink_0())
                    .child(
                        div().flex_1().min_w_0().truncate().text_sm().child(
                            rust_i18n::t!(
                                "tunnel_route",
                                local = tunnel.local_port,
                                remote = tunnel.remote_port
                            )
                            .to_string(),
                        ),
                    )
                    .child(
                        Button::new(("tunnel-open", id))
                            .ghost()
                            .small()
                            .icon(IconName::Globe)
                            .disabled(!state.loaded || state.error.is_some())
                            .tooltip(tr("tunnel_open"))
                            .accessibility_label(tr("tunnel_open"))
                            .debug_selector(move || format!("tunnel-open-{id}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.tunnel_preview(host, id, &opened, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("tunnel-close", id))
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .disabled(!state.loaded || state.error.is_some())
                            .tooltip(tr("tunnel_close"))
                            .accessibility_label(tr("tunnel_close"))
                            .debug_selector(move || format!("tunnel-close-{id}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(state) = this.workspace.tunnels.get_mut(&host) {
                                    state.close(id, &closed);
                                }
                                cx.notify();
                            })),
                    )
            }))
            .into_any_element()
    }

    fn tunnel_preview(
        &self,
        host: usize,
        id: usize,
        expected: &Tunnel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.workspace.tunnels.get(&host) else {
            return;
        };
        if !state.loaded
            || state.error.is_some()
            || expected.owner.host != host
            || state.entries.get(&id) != Some(expected)
            || !self.workspace.contains(expected.owner)
        {
            return;
        }
        let url = format!("http://localhost:{}", expected.local_port);
        crate::feedback::info(
            &tr("tunnel_open"),
            &format!("{}\n{url}", tr("tunnel_open_hint")),
            window,
            cx,
        );
    }
}
