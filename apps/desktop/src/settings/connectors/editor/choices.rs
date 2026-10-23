//! Connector choices reuse their package's localized presentation metadata.
use super::*;

impl Editor {
    pub(super) fn choices(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.query.read(cx).value().to_lowercase();
        let owner = self.owner.read(cx);
        let catalog = &owner.plugin_catalog;
        let metadata = catalog.metadata.as_ref().map(|state| state.read(cx));
        let rows = self
            .types
            .iter()
            .enumerate()
            .filter_map(|(index, kind)| {
                let info = metadata
                    .and_then(|metadata| metadata.entries.get(&kind.package.name))
                    .filter(|info| info.summary.reference() == kind.package);
                let manual = info.is_some_and(|info| info.mcp_source.is_some());
                let added = manual || existing(kind, &owner.connectors.rows).is_some();
                let label = if manual {
                    crate::plugins::metadata::title(info.expect("manual instance"))
                } else {
                    title(&kind.declaration.display)
                };
                let description = metadata
                    .and_then(|metadata| metadata.entries.get(&kind.package.name))
                    .filter(|info| info.summary.digest == kind.package.digest)
                    .and_then(crate::plugins::metadata::description)
                    .or_else(|| {
                        catalog
                            .packages
                            .iter()
                            .find(|summary| {
                                summary.name == kind.package.name
                                    && summary.digest == kind.package.digest
                            })
                            .and_then(|summary| summary.description.clone())
                    })
                    .filter(|description| !description.trim().is_empty());
                if !label.to_lowercase().contains(&query)
                    && !description
                        .as_ref()
                        .is_some_and(|description| description.to_lowercase().contains(&query))
                {
                    return None;
                }
                let icon = kind
                    .declaration
                    .display
                    .icon
                    .as_ref()
                    .map(crate::assets::icons::icon)
                    .unwrap_or_else(|| Icon::new(gpui_kit::assets::IconName::Plug));
                let accessibility = match &description {
                    Some(description) => format!("{label}: {description}"),
                    None => label.clone(),
                };
                Some(
                    Button::new(("connector-type", index))
                        .outline()
                        .w_full()
                        .h_auto()
                        .p_3()
                        .justify_start()
                        .disabled(self.pending || (!kind.enabled && !added))
                        .accessibility_label(accessibility)
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_3()
                                .child(
                                    crate::plugins::emblem::surface(&label, None, px(32.))
                                        .flex_shrink_0()
                                        .child(icon.size_5()),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .items_start()
                                        .gap_1()
                                        .child(div().w_full().truncate().child(label))
                                        .when_some(description, |element, description| {
                                            element.child(
                                                div()
                                                    .w_full()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(description),
                                            )
                                        }),
                                )
                                .when(added, |row| {
                                    row.child(
                                        Tag::secondary().xsmall().child(tr("connector_added")),
                                    )
                                }),
                        )
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.pick(index, window, cx)),
                        ),
                )
            })
            .collect::<Vec<_>>();
        v_flex()
            .w_full()
            .gap_4()
            .child(div().text_lg().font_semibold().child(tr("connector_add")))
            .child(Input::new(&self.query).cleanable(true))
            .child(
                v_flex()
                    .id("connector-type-list")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .gap_2()
                    .when(rows.is_empty(), |element| {
                        element.child(crate::empty_state::list(
                            IconName::Network,
                            "connector_empty",
                            cx,
                        ))
                    })
                    .children(rows),
            )
            .into_any_element()
    }
    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(kind) = self.types.get(index).cloned() else {
            return;
        };
        let info = self
            .owner
            .read(cx)
            .plugin_catalog
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.read(cx).entries.get(&kind.package.name))
            .filter(|info| info.summary.reference() == kind.package && info.mcp_source.is_some())
            .cloned();
        if let Some(info) = info
            && let Some(server) = info.mcp.first().map(|server| server.name.clone())
        {
            let owner = self.owner.clone();
            let binding = self.binding.clone();
            self.closed = true;
            self.stop.cancel();
            window.close_dialog(cx);
            window.defer(cx, move |window, cx| {
                super::super::super::mcp::configure_server(
                    owner, binding, info, server, window, cx,
                );
            });
        } else {
            self.choose(index, window, cx);
        }
    }
}

pub(super) fn existing<'a>(kind: &Type, accounts: &'a [Connection]) -> Option<&'a Connection> {
    accounts.iter().find(|account| {
        account.package.name == kind.package.name && account.connector == kind.declaration.id
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn added_services_resolve_the_original_account() {
        let package =
            serde_json::json!({"name":"service-id","digest":"current","settings_revision":9});
        let kind: Type = serde_json::from_value(serde_json::json!({
            "package":package,"declaration":{"id":"account-id","display":{"label":"Shared name"},
                "accounts":"single","origins":["https://example.com"],
                "auth":[{"kind":"secret","fields":["token"]}],"fields":[],"inject":[]}
        }))
        .unwrap();
        let mut account: Connection = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::RequestId::new(),"package":package,"connector":"account-id",
            "method":"secret","revision":2,"enabled":false,"state":"expired",
            "identity":null,"expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":[]
        }))
        .unwrap();
        account.package.digest = "previous".into();
        let original = account.id;
        let mut other = account.clone();
        other.id = sailry_protocol::RequestId::new();
        other.package.name = "other-service-id".into();
        let accounts = [other, account];
        assert_eq!(existing(&kind, &accounts).unwrap().id, original);
        let mut different = kind.clone();
        different.declaration.id = "other-account-id".into();
        assert!(existing(&different, &accounts).is_none());
    }
}
