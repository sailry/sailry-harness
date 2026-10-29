use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

impl Workspace {
    pub(super) fn provider_menu(
        &self,
        channel: &Channel,
        authentication: Authentication,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let owner = cx.entity();
        let real = self.provider_link.is_some();
        let login = (real && authentication != Authentication::ApiKey).then_some(
            if channel.credential_configured {
                "provider_reconnect"
            } else {
                "provider_connect"
            },
        );
        let removal = self.provider_link.as_ref().and_then(|live| {
            live.providers.get(&id).map(|provider| {
                (
                    live.binding.clone(),
                    sailry_protocol::Command::RemoveProvider {
                        provider: provider.id,
                        expected_revision: provider.revision,
                    },
                )
            })
        });
        Button::new(("provider-more", id))
            .ghost()
            .small()
            .flex_shrink_0()
            .debug_selector(move || format!("provider-more-{id}"))
            .icon(IconName::Ellipsis)
            .accessibility_label(tr("more"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let edit = owner.clone();
                let menu = menu.min_w(px(160.)).item(
                    PopupMenuItem::new(tr("settings_edit"))
                        .disabled(busy)
                        .on_click(move |_, window, cx| {
                            editor::open(edit.clone(), Some(id), None, window, cx);
                        }),
                );
                let menu = match login {
                    Some(key) => {
                        let owner = owner.clone();
                        menu.item(PopupMenuItem::new(tr(key)).disabled(busy).on_click(
                            move |_, window, cx| {
                                login::open(owner.clone(), id, window, cx);
                            },
                        ))
                    }
                    None => menu,
                };
                let owner = owner.clone();
                let removal = removal.clone();
                menu.separator().item(
                    PopupMenuItem::element(|_, cx| {
                        div()
                            .text_color(cx.theme().danger)
                            .child(tr("settings_delete"))
                    })
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        remove(owner.clone(), id, real, removal.clone(), window, cx)
                    }),
                )
            })
            .into_any_element()
    }
}

fn remove(
    owner: Entity<Workspace>,
    id: usize,
    real: bool,
    removal: Option<(Binding, sailry_protocol::Command)>,
    window: &mut Window,
    cx: &mut App,
) {
    crate::prompts::confirm(
        &tr("provider_delete_title"),
        &tr(if real {
            "provider_remove_effect"
        } else {
            "settings_remove_preview"
        }),
        tr("settings_delete"),
        window,
        cx,
        move |_, cx| {
            owner.update(cx, |this, cx| {
                if let Some((binding, command)) = &removal {
                    this.submit_provider(binding.clone(), command.clone(), cx);
                } else {
                    this.providers.remove(id);
                }
                cx.notify();
            })
        },
    );
}
