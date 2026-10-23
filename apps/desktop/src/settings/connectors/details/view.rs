use super::*;
use crate::settings::group::{Group, Row};

use crate::theme::DialogStyle as _;

pub(super) fn open(details: Entity<Details>, window: &mut Window, cx: &mut App) {
    if !details.read(cx).active(cx) {
        return;
    }
    let active = details.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = details.clone();
        let state = details.read(cx);
        let title = state
            .service(cx)
            .map(|service| service.label())
            .unwrap_or_else(|| state.package.clone());
        dialog
            .form_title(title, cx)
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .max_h(window.viewport_size().height * 0.85)
            .on_close(move |_, _, cx| {
                close.update(cx, |details, _| details.dialog.dismissed());
            })
            .child(details.clone())
    });
    active.update(cx, |details, cx| {
        crate::ui::overlays::Owned::bind(details, |details| &mut details.dialog, window, cx)
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(service) = self.service(cx) else {
            return v_flex()
                .w_full()
                .when(self.loading(cx), |content| content.child(Spinner::new()))
                .when(!self.loading(cx), |content| {
                    content.child(Tag::new().child(tr("connector_unavailable")))
                })
                .into_any_element();
        };
        v_flex()
            .id("connector-details")
            .w_full()
            .gap_4()
            .text_sm()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.binding.label.clone()),
            )
            .when(!service.enabled(), |content| {
                content.child(Tag::new().child(tr("connector_package_disabled")))
            })
            .when(
                service.kind.is_some() || service.account.is_some(),
                |content| content.child(self.authentication(&service, cx)),
            )
            .children(
                service
                    .servers
                    .iter()
                    .map(|server| self.server(&service, server, cx)),
            )
            .when(
                service.info.is_some() && service.servers.is_empty(),
                |content| content.child(self.api_tools(&service, cx)),
            )
            .into_any_element()
    }
}

impl Details {
    fn authentication(&self, service: &catalog::Service, cx: &mut Context<Self>) -> AnyElement {
        let mut group = Group::new("connector_authentication");
        if let Some(account) = &service.account {
            if let Some(owner) = self.owner.upgrade() {
                let row = owner.update(cx, |owner, cx| owner.connector_row(account, cx));
                group = group.child(row);
            }
        } else if let Some(kind) = &service.kind {
            let owner = self.owner.clone();
            let kind = kind.clone();
            let label = kind
                .declaration
                .auth
                .iter()
                .map(|auth| tr(filter::key(auth.kind())).to_string())
                .collect::<Vec<_>>()
                .join(" · ");
            group = group.child(Row::with_label(
                "connector-auth-methods",
                label,
                Button::new("connector-connect")
                    .small()
                    .label(tr("connector_connect"))
                    .disabled(!service.enabled())
                    .on_click(move |_, window, cx| {
                        if let Some(owner) = owner.upgrade() {
                            editor::connect(owner, kind.clone(), window, cx);
                        }
                    }),
            ));
        }
        group.into_any_element()
    }

    fn server(
        &self,
        service: &catalog::Service,
        server: &Mcp,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = server.name.clone();
        let allowed_server = server.clone();
        let retry_server = server.clone();
        let check_server = server.clone();
        let configure_server = name.clone();
        let configure_info = service.info.clone();
        let configure_owner = cx.entity();
        let busy = self.pending.contains(&name);
        let draft = self.drafts.get(&name).copied();
        let uncertain = self.requests.contains_key(&name);
        let label = availability(server.availability).unwrap_or("connector_available");
        let controls = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new(format!("connector-configure-{name}"))
                    .ghost()
                    .small()
                    .label(tr("connector_configure"))
                    .disabled(configure_info.is_none() || busy)
                    .on_click(move |_, window, cx| {
                        if let Some(info) = &configure_info {
                            configure_owner.update(cx, |details, cx| {
                                details.configure(
                                    info.clone(),
                                    configure_server.clone(),
                                    window,
                                    cx,
                                )
                            });
                        }
                    }),
            )
            .child(
                Button::new(format!("connector-check-{name}"))
                    .small()
                    .label(tr("connector_probe"))
                    .loading(busy)
                    .disabled(busy || !checkable(service, server))
                    .on_click(cx.listener(move |details, _, window, cx| {
                        details.check(check_server.clone(), window, cx)
                    })),
            )
            .child(
                Switch::new(format!("connector-allowed-{name}"))
                    .checked(draft.unwrap_or(server.allowed))
                    .disabled(busy || uncertain)
                    .accessibility_label(tr("connector_server_allowed"))
                    .on_click(cx.listener(move |details, value, window, cx| {
                        details.set_allowed(allowed_server.clone(), *value, window, cx)
                    })),
            )
            .when(draft.is_some(), |row| {
                row.child(Tag::new().xsmall().child(tr("connector_unsaved")))
            })
            .when(draft.is_some() && !busy, |row| {
                row.child(
                    Button::new(format!("connector-allowed-retry-{name}"))
                        .small()
                        .label(tr(if uncertain {
                            "plugins_retry"
                        } else {
                            "settings_save"
                        }))
                        .on_click(cx.listener(move |details, _, window, cx| {
                            details.set_allowed(
                                retry_server.clone(),
                                draft.expect("visible draft"),
                                window,
                                cx,
                            )
                        })),
                )
            });
        let group = Group::new("mcp_configuration").child(
            super::super::super::entry::Entry::new(
                format!("connector-server-{name}"),
                status_label(&name, label, cx),
            )
            .control(controls),
        );
        let checked = self
            .checks
            .get(&name)
            .filter(|checked| checked.current(service))
            .map(|checked| &checked.result);
        let tools = Group::new("plugins_tools")
            .empty(
                IconName::Settings2,
                if checked.is_some() {
                    "connector_tools_empty"
                } else {
                    "connector_tools_unchecked"
                },
            )
            .children(
                checked
                    .into_iter()
                    .flat_map(|checked| &checked.tools)
                    .map(|tool| {
                        tool_row(
                            format!("connector-tool-{name}-{}", tool.name),
                            &tool.name,
                            tool.description.as_deref(),
                            cx,
                        )
                    }),
            );
        v_flex()
            .id(SharedString::from(format!(
                "connector-server-section-{name}"
            )))
            .gap_4()
            .child(group)
            .child(tools)
            .into_any_element()
    }

    fn api_tools(&self, service: &catalog::Service, cx: &App) -> AnyElement {
        Group::new("plugins_tools")
            .empty(IconName::Settings2, "connector_tools_empty")
            .children(service.tools().map(|tool| {
                tool_row(
                    format!("connector-api-tool-{}", tool.name),
                    &tool.name,
                    tool.description.as_deref(),
                    cx,
                )
            }))
            .into_any_element()
    }
}

pub(super) fn checkable(service: &catalog::Service, server: &Mcp) -> bool {
    use sailry_protocol::plugin::mcp::Availability;
    service.enabled()
        && matches!(
            server.availability,
            Availability::Available | Availability::ServerDisabled
        )
}

fn status_label(name: &str, status: &'static str, cx: &App) -> AnyElement {
    v_flex()
        .gap_1()
        .child(name.to_owned())
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(tr(status)),
        )
        .into_any_element()
}

fn tool_row(id: String, name: &str, description: Option<&str>, cx: &App) -> AnyElement {
    super::super::super::entry::Entry::new(
        id,
        v_flex()
            .gap_1()
            .child(name.to_owned())
            .when_some(description, |row, description| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description.to_owned()),
                )
            }),
    )
    .into_any_element()
}
