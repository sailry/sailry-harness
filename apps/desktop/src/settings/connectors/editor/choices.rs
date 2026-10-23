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
            .filter(|(_, kind)| {
                !owner.connectors.rows.iter().any(|row| {
                    row.package.name == kind.package.name && row.connector == kind.declaration.id
                })
            })
            .filter_map(|(index, kind)| {
                let label = title(&kind.declaration.display);
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
                        .disabled(self.pending || !kind.enabled)
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
                                ),
                        )
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.choose(index, window, cx)),
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
}
