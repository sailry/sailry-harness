use super::entry::Entry;
mod catalog;
mod data;
mod directory;
mod editor;
mod families;
mod list;
mod live;
mod login;
mod model;
mod presets;
mod vendors;
use gpui_kit::prelude::FluentBuilder as _;
pub(super) use live::Binding;
pub(super) use live::Live;
pub(super) use live::local as bind_local;
pub(crate) use live::{channel, logo};

use super::{
    Workspace,
    group::{Group, Row},
};
use crate::tr;
pub(crate) use data::Category as ModelCategory;
use data::Preset;
pub(super) use data::Store;
pub(crate) use data::{Channel, Model};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    switch::Switch,
    *,
};
use gpui_kit::*;

impl Workspace {
    pub(crate) fn provider_brands(
        entries: Option<&[sailry_protocol::conversation::presets::Entry]>,
    ) -> Vec<(String, SharedString, AnyElement)> {
        editor::Editor::brands(entries)
    }

    pub(crate) fn add_provider(
        owner: Entity<Self>,
        brand: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = editor::open(owner, None, window, cx);
        if let Some(brand) = brand {
            editor.update(cx, |editor, cx| editor.choose_brand(brand, window, cx));
        }
    }

    pub(super) fn providers(&self, cx: &mut Context<Self>) -> AnyElement {
        let category = &self.providers.category;
        let real = self.provider_link.is_some();
        let busy = self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.pending || !live.connected);
        let owner = cx.entity();
        let mut rows = Group::new("provider_channels")
            .heading(false)
            .empty(IconName::Cpu, "provider_empty_live");
        let matching = self.providers.channels.iter().filter(|channel| {
            let provider = self
                .provider_link
                .as_ref()
                .and_then(|live| live.providers.get(&channel.id));
            *category == data::Selection::All || families::selection(channel, provider) == *category
        });
        for channel in matching {
            let id = channel.id;
            let authorization = (real
                && self
                    .provider_link
                    .as_ref()
                    .and_then(|live| live.providers.get(&id))
                    .is_some_and(|provider| {
                        provider.authentication != sailry_protocol::Authentication::ApiKey
                    }))
            .then(|| {
                let owner = owner.clone();
                Button::new(("provider-login", id))
                    .ghost()
                    .small()
                    .disabled(busy)
                    .debug_selector(move || format!("provider-login-{id}"))
                    .icon(IconName::CircleUser)
                    .tooltip(tr("provider_connect"))
                    .accessibility_label(tr("provider_connect"))
                    .on_click(move |_, window, cx| {
                        login::open(owner.clone(), id, window, cx);
                    })
            });
            let edit_owner = owner.clone();
            let remove_owner = owner.clone();
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
            rows = rows.child(
                Entry::new(format!("provider-{id}"), list::content(channel, real, cx))
                    .control_width(rems(3.))
                    // Four small Kit buttons and three gaps share the fixed action slot.
                    .actions_width(rems(7.5))
                    .control(
                        h_flex().child(
                            Switch::new(("provider-enabled", id))
                                .disabled(busy)
                                .checked(channel.enabled)
                                .accessibility_label(tr("provider_enabled"))
                                .on_click(cx.listener(move |this, enabled, _, cx| {
                                    if this.provider_link.is_some() {
                                        this.provider_action(id, false, *enabled, cx);
                                        return;
                                    }
                                    if let Some(channel) =
                                        this.providers.channels.iter_mut().find(|c| c.id == id)
                                    {
                                        channel.enabled = *enabled;
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .when_some(authorization, |row, button| row.action(button))
                    .action(
                        Button::new(("provider-edit", id))
                            .disabled(busy)
                            .debug_selector(move || format!("provider-edit-{id}"))
                            .icon(IconName::Settings2)
                            .tooltip(tr("settings_edit"))
                            .accessibility_label(tr("settings_edit"))
                            .on_click(move |_, window, cx| {
                                editor::open(edit_owner.clone(), Some(id), window, cx);
                            }),
                    )
                    .action(
                        Button::new(("provider-delete", id))
                            .disabled(busy)
                            .debug_selector(move || format!("provider-delete-{id}"))
                            .icon(IconName::CircleX)
                            .tooltip(tr("settings_delete"))
                            .accessibility_label(tr("settings_delete"))
                            .on_click(move |_, window, cx| {
                                let owner = remove_owner.clone();
                                let removal = removal.clone();
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
                                                this.submit_provider(
                                                    binding.clone(),
                                                    command.clone(),
                                                    cx,
                                                );
                                            } else {
                                                this.providers.remove(id);
                                            }
                                            cx.notify();
                                        })
                                    },
                                );
                            }),
                    ),
            );
        }
        v_flex()
            .gap_6()
            .child(self.model_catalog(cx))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .debug_selector(|| "settings-heading-provider_channels".into())
                            .w_full()
                            .min_w_0()
                            .min_h_8()
                            .gap_3()
                            .child(
                                h_flex()
                                    .debug_selector(|| "settings-title-provider_channels".into())
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.provider_families(cx)),
                            )
                            .child(
                                h_flex()
                                    .debug_selector(|| "settings-actions-provider_channels".into())
                                    .flex_shrink_0()
                                    .child(
                                        Button::new("settings-add")
                                            .disabled(busy)
                                            .primary()
                                            .debug_selector(|| "settings-add".into())
                                            .label(tr("settings_add"))
                                            .icon(IconName::Plus)
                                            .on_click(move |_, window, cx| {
                                                Self::add_provider(owner.clone(), None, window, cx);
                                            }),
                                    ),
                            ),
                    )
                    .child(rows),
            )
            .into_any_element()
    }
}
