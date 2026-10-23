use super::*;
use crate::settings::group::{Group, Row};

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(service) = self.service(cx) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .when(self.loading(cx), |content| content.child(Spinner::new()))
                .when(!self.loading(cx), |content| {
                    content.child(Tag::new().child(tr("connector_unavailable")))
                })
                .into_any_element();
        };
        let label = service.label();
        let host = self.binding.label.clone();
        let enabled = service.enabled();
        let source = service.info.clone();
        let package_owner = self.owner.clone();
        let remove_owner = self.owner.clone();
        let package_node = self.binding.client.target();
        let package_info = source.clone();
        let busy = self.owner.read(cx).plugin_catalog.busy(cx);
        let package =
            Group::new("settings_plugins")
                .heading(false)
                .when_some(source, |group, info| {
                    let summary = info.summary.clone();
                    let remove = summary.clone();
                    group.child(Row::with_label(
                        "connector-package",
                        crate::plugins::metadata::title(&info),
                        h_flex()
                            .gap_2()
                            .when(info.mcp_source.is_none(), |controls| {
                                controls.child(
                                    Button::new("connector-view-plugin")
                                        .ghost()
                                        .small()
                                        .label(tr("connector_view_plugin"))
                                        .on_click(move |_, window, cx| {
                                            if let Some(info) = &package_info {
                                                window.dispatch_action(
                                                    Box::new(crate::settings::OpenManagement {
                                                        node: package_node,
                                                        name: info.summary.name.clone(),
                                                        skill: None,
                                                    }),
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                            })
                            .when(info.mcp_source.is_some(), |controls| {
                                controls.child(
                                    Button::new("connector-remove-mcp")
                                        .ghost()
                                        .small()
                                        .label(tr("plugins_uninstall"))
                                        .disabled(busy)
                                        .on_click(move |_, window, cx| {
                                            if remove_owner
                                                .read(cx)
                                                .provider_link
                                                .as_ref()
                                                .is_some_and(|live| {
                                                    live.binding.client.target() == package_node
                                                })
                                            {
                                                super::super::super::plugins::dialog::open(
                                                    remove_owner.clone(),
                                                    Some(remove.clone()),
                                                    true,
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                            })
                            .child(
                                Switch::new("connector-package-enabled")
                                    .checked(info.summary.enabled)
                                    .disabled(busy)
                                    .accessibility_label(tr("plugins_enabled"))
                                    .on_click(move |value, window, cx| {
                                        package_owner.update(cx, |owner, cx| {
                                            owner.toggle_plugin(&summary, *value, window, cx)
                                        });
                                    }),
                            ),
                    ))
                });
        v_flex()
            .id("connector-details")
            .size_full()
            .min_h_0()
            .text_sm()
            .child(
                v_flex()
                    .w_full()
                    .px_7()
                    .gap_2()
                    .child(div().text_lg().font_semibold().child(label))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(host),
                    )
                    .when(!enabled, |header| {
                        header.child(Tag::new().child(tr("connector_package_disabled")))
                    }),
            )
            .child(
                v_flex()
                    .id("connector-details-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_7()
                    .gap_5()
                    .when(service.info.is_some(), |content| content.child(package))
                    .when(service.info.is_none(), |content| {
                        content
                            .when(self.loading(cx), |content| {
                                content.child(Spinner::new().small())
                            })
                            .when(!self.loading(cx), |content| {
                                content.child(Tag::new().child(tr("connector_unavailable")))
                            })
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
                    ),
            )
            .into_any_element()
    }
}

impl Details {
    fn authentication(&self, service: &catalog::Service, cx: &mut Context<Self>) -> AnyElement {
        let mut group = Group::new("connector_authentication");
        if let Some(account) = &service.account {
            let row = self
                .owner
                .update(cx, |owner, cx| owner.connector_row(account, cx));
            group = group.child(row);
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
                        editor::connect(owner.clone(), kind.clone(), window, cx);
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
            .filter(|checked| checked.package == service.package);
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
