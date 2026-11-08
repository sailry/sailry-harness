use super::super::status::{self, Status};
use super::*;
use crate::{theme::DialogStyle as _, ui::media_row::MediaRow};
use gpui_kit::component::{
    description_list::{DescriptionItem, DescriptionList},
    dialog::{DialogClose, DialogFooter},
};
use sailry_protocol::plugin::mcp::{Availability, RuntimeState};

pub(super) fn open(details: Entity<Details>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = details.read(cx);
        let servers = state
            .service(cx)
            .is_some_and(|service| !service.servers.is_empty());
        let ready = !state.reconnectable(cx).is_empty();
        let reconnect = details.clone();
        dialog
            .floating_surface(cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.85)
            .p_6()
            .child(details.clone())
            .footer(
                DialogFooter::new()
                    .gap_2()
                    // Keep Kit's full-width close wrapper in an intrinsic slot.
                    .child(
                        h_flex()
                            .w_auto()
                            .flex_shrink_0()
                            .child(DialogClose::new().trigger(|button| button.label(tr("close")))),
                    )
                    .when(servers, |footer| {
                        footer.child(
                            Button::new("connector-reconnect")
                                .primary()
                                .label(tr("connector_mcp_reconnect"))
                                .disabled(!ready)
                                .on_click(move |_, window, cx| {
                                    reconnect
                                        .update(cx, |details, cx| details.reconnect(window, cx))
                                }),
                        )
                    }),
            )
    });
}

impl Details {
    /// Servers that can be checked now and have no command in flight.
    fn reconnectable(&self, cx: &App) -> Vec<Mcp> {
        self.service(cx)
            .map(|service| {
                service
                    .servers
                    .iter()
                    .filter(|server| {
                        checkable(&service, server) && !self.pending.contains(&server.name)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for server in self.reconnectable(cx) {
            self.check(server, window, cx);
        }
    }
}

/// One line for the whole service: its aggregate status, else the list description.
fn summary(service: &catalog::Service) -> SharedString {
    match status::service(service) {
        Some(Status::Connected(tools)) => format!(
            "{} · {}",
            tr("connector_connected"),
            rust_i18n::t!("connector_mcp_tools", n = tools)
        )
        .into(),
        Some(Status::Connecting) => tr("connector_mcp_connecting"),
        Some(Status::Failed) => tr("connector_mcp_failed"),
        Some(Status::Problem(key)) => tr(key),
        None => super::super::description(service).into(),
    }
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
        // A single server's status is already the header summary.
        let several = service.servers.len() > 1;
        let mut facts = DescriptionList::new()
            .columns(1)
            .bordered(true)
            .label_width(rems(6.));
        for (index, server) in service.servers.iter().enumerate() {
            if index > 0 {
                facts = facts.separator();
            }
            facts = facts.child(
                DescriptionItem::new(tr("connector_mcp_server")).value(
                    div()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(server.name.clone())
                        .into_any_element(),
                ),
            );
            if several {
                facts = facts.child(
                    DescriptionItem::new(tr("connector_mcp_status"))
                        .value(status::view(status::server(enabled, server), cx)),
                );
            }
            let Some(runtime) = server
                .runtime
                .as_ref()
                .filter(|_| enabled && !status::disabled(server))
            else {
                continue;
            };
            // Tools stay visible when the status line no longer reports them.
            if runtime.tools > 0
                && (runtime.state != RuntimeState::Connected
                    || server.availability != Availability::Available)
            {
                facts = facts
                    .child(DescriptionItem::new(tr("market_details_tools")).value(
                        rust_i18n::t!("connector_mcp_tools", n = runtime.tools).to_string(),
                    ));
            }
            if let Some(updated) = status::updated(server) {
                facts = facts
                    .child(DescriptionItem::new(tr("connector_mcp_updated_at")).value(updated));
            }
            if let Some(fault) = &runtime.fault {
                facts = facts.child(
                    DescriptionItem::new(tr("connector_mcp_error")).value(
                        div()
                            .whitespace_normal()
                            .child(fault.message.clone())
                            .into_any_element(),
                    ),
                );
            }
        }
        v_flex()
            .gap_4()
            .child(
                MediaRow::new(service.label())
                    .leading(super::super::tile(&service))
                    .description(summary(&service)),
            )
            .when(!service.servers.is_empty(), |content| content.child(facts))
            .into_any_element()
    }
}
