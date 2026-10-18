use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

impl Editor {
    pub(super) fn kind_control(&self, cx: &Context<Self>) -> impl IntoElement {
        if let Some(entry) = self.selected.as_ref() {
            let mut choices = entry
                .preset
                .routes
                .iter()
                .filter(|route| {
                    self.selected_route().is_some_and(|current| {
                        route.region == current.region && route.product == current.product
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            choices.dedup_by_key(|route| route.protocol);
            let current = self.selected_route().cloned();
            let owner = cx.entity();
            return h_flex()
                .gap_2()
                .when(choices.len() > 1, |row| {
                    row.child(
                        Button::new("provider-api")
                            .w(rems(9.))
                            .label(current.as_ref().map_or_else(
                                || tr("provider_api"),
                                |route| protocol_label(route.protocol),
                            ))
                            .dropdown_caret(true)
                            .disabled(self.navigation_blocked() || self.editing.is_some())
                            .dropdown_menu(move |menu, _, _| {
                                choices.iter().fold(menu, |menu, route| {
                                    let route = route.clone();
                                    let owner = owner.clone();
                                    menu.item(
                                        PopupMenuItem::new(protocol_label(route.protocol))
                                            .checked(current.as_ref() == Some(&route))
                                            .on_click(move |_, window, cx| {
                                                owner.update(cx, |editor, cx| {
                                                    editor.change_route(route.clone(), window, cx)
                                                })
                                            }),
                                    )
                                })
                            }),
                    )
                })
                .into_any_element();
        }
        let preset = self.preset;
        let disabled = self.editing.is_some() || self.provider.is_some() || self.pending;
        h_flex()
            .debug_selector(|| "provider-kind-row".into())
            .flex_shrink_0()
            .gap_2()
            .when(!preset.protocols().is_empty(), |row| {
                let owner = cx.entity();
                row.child(
                    Button::new("provider-api")
                        .debug_selector(|| "provider-api".into())
                        .w(rems(9.))
                        .flex_shrink_0()
                        .label(tr(preset.protocol_key()))
                        .accessibility_label(tr("provider_api"))
                        .dropdown_caret(true)
                        .disabled(disabled)
                        .dropdown_menu(move |menu, _, _| {
                            preset.protocols().iter().fold(menu, |menu, &p| {
                                let owner = owner.clone();
                                menu.item(
                                    PopupMenuItem::new(tr(p.protocol_key()))
                                        .checked(p == preset)
                                        .on_click(move |_, window, cx| {
                                            owner.update(cx, |editor, cx| {
                                                editor.select_preset(p, window, cx)
                                            })
                                        }),
                                )
                            })
                        }),
                )
            })
            .into_any_element()
    }
}

fn protocol_label(protocol: sailry_protocol::conversation::presets::Protocol) -> SharedString {
    use sailry_protocol::conversation::presets::Protocol;
    tr(match protocol {
        Protocol::OpenAiChatCompletions => "provider_api_chat",
        Protocol::OpenAiResponses => "provider_api_responses",
        _ => "provider_api",
    })
}
