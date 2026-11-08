use super::*;
use crate::theme::DialogStyle as _;

pub(super) fn open(details: Entity<Details>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .floating_surface(cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.85)
            .p_6()
            .child(details.clone())
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(service) = self.service(cx) else {
            return div().into_any_element();
        };
        let enabled = service.enabled()
            && service
                .account
                .as_ref()
                .is_none_or(|account| account.enabled);
        v_flex()
            .gap_4()
            .child(div().text_lg().font_semibold().child(service.label()))
            .children(service.servers.iter().map(|server| {
                let selected = server.clone();
                let check_enabled =
                    checkable(&service, server) && !self.pending.contains(&server.name);
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(server.name.clone())
                            .child(
                                Button::new(format!("mcp-reconnect-{}", server.name))
                                    .small()
                                    .ghost()
                                    .label(tr("connector_mcp_reconnect"))
                                    .disabled(!check_enabled)
                                    .on_click(cx.listener(move |details, _, window, cx| {
                                        details.check(selected.clone(), window, cx)
                                    })),
                            ),
                    )
                    .child(super::super::status::view(
                        super::super::status::server(enabled, server),
                        cx,
                    ))
                    .child(super::super::status::information(enabled, server, cx))
            }))
            .into_any_element()
    }
}
