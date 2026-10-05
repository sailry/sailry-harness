use super::entry::Entry;
#[cfg(test)]
use crate::provider_fixture as authorization_support;
use crate::theme::DialogStyle as _;
mod catalog;
mod data;
mod families;
mod vendors;
#[cfg(test)]
use crate::discovery_fixture as discovery_server;
mod editor;
#[cfg(test)]
pub(in crate::settings) mod fixture;
mod live;
mod login;
mod model;
use gpui_kit::prelude::FluentBuilder as _;
pub(super) use live::Binding;
pub(super) use live::Live;
pub(crate) use live::channel;
pub(super) use live::local as bind_local;

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
    pub(super) fn providers(&self, cx: &mut Context<Self>) -> AnyElement {
        let category = self.providers.category;
        let real = self.provider_link.is_some();
        let busy = self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.pending || !live.connected);
        let owner = cx.entity();
        let mut rows = Group::new("provider_channels")
            .heading(false)
            .empty(IconName::Cpu, "provider_empty_live");
        let matching = self
            .providers
            .channels
            .iter()
            .filter(|channel| channel.preset.category() == category);
        for channel in matching {
            let id = channel.id;
            let edit_owner = owner.clone();
            let remove_owner = owner.clone();
            let oauth_owner = owner.clone();
            let login_label = tr(if channel.credential_configured || channel.connected {
                "provider_reconnect"
            } else {
                "provider_connect"
            });
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
                Entry::new(
                    format!("provider-{id}"),
                    v_flex()
                        .gap_1()
                        .child(div().truncate().child(channel.name.clone()))
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_3()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    div()
                                        .debug_selector(move || format!("provider-kind-{id}"))
                                        .min_w_0()
                                        .truncate()
                                        .child(tr(channel.preset.key())),
                                )
                                .child(
                                    div()
                                        .debug_selector(move || format!("provider-count-{id}"))
                                        .flex_shrink_0()
                                        .child(
                                            rust_i18n::t!(
                                                "provider_model_count",
                                                count = channel.models.len()
                                            )
                                            .to_string(),
                                        ),
                                )
                                .when(!real, |summary| {
                                    summary.child(div().flex_shrink_0().child(tr("preview")))
                                }),
                        ),
                )
                .control(
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
                )
                .when(channel.preset.oauth(), |row| {
                    row.action(
                        Button::new(("provider-oauth", id))
                            .disabled(busy)
                            .debug_selector(move || format!("provider-login-{id}"))
                            .icon(
                                Icon::default()
                                    .path("reicon:arrows-action/login")
                                    .text_color(cx.theme().success),
                            )
                            .tooltip(login_label.clone())
                            .accessibility_label(login_label)
                            .on_click(move |_, window, cx| {
                                if real {
                                    login::open(oauth_owner.clone(), id, window, cx);
                                } else {
                                    oauth(oauth_owner.clone(), id, window, cx);
                                }
                            }),
                    )
                })
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
                                                editor::open(owner.clone(), None, window, cx);
                                            }),
                                    ),
                            ),
                    )
                    .child(rows),
            )
            .into_any_element()
    }
}

fn oauth(owner: Entity<Workspace>, id: usize, window: &mut Window, cx: &mut App) {
    let connected = owner
        .read(cx)
        .providers
        .channels
        .iter()
        .any(|c| c.id == id && c.connected);
    window.open_dialog(cx, move |dialog, _, _| {
        let owner = owner.clone();
        dialog
            .form_title(tr(if connected {
                "provider_reconnect"
            } else {
                "provider_connect"
            }))
            .child(tr("provider_oauth_preview"))
            .when(!connected, |dialog| {
                dialog.child(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_xl()
                                .font_semibold()
                                .child(tr("provider_oauth_code")),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("oauth-open-verification")
                                        .primary()
                                        .label(tr("provider_oauth_open"))
                                        .disabled(true)
                                        .tooltip(tr("provider_oauth_preview")),
                                )
                                .child(
                                    Button::new("oauth-copy-code")
                                        .label(tr("provider_oauth_copy"))
                                        .on_click(|_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                tr("provider_oauth_code").to_string(),
                                            ))
                                        }),
                                ),
                        )
                        .child(div().text_sm().child("https://example.invalid/device")),
                )
            })
            .footer(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .child(
                        Button::new("oauth-preview-confirm")
                            .primary()
                            .label(tr("provider_oauth_simulate"))
                            .on_click(move |_, window, cx| {
                                owner.update(cx, |this, cx| {
                                    if let Some(channel) =
                                        this.providers.channels.iter_mut().find(|c| c.id == id)
                                    {
                                        channel.connected = true;
                                    }
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}
