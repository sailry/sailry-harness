use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

impl Workspace {
    pub(super) fn connector_menu(&self, row: &Connection, cx: &mut Context<Self>) -> AnyElement {
        let row = row.clone();
        let owner = cx.entity();
        let id = row.id.to_string();
        Button::new(format!("connector-menu-{id}"))
            .disabled(
                self.connectors.pending.contains(&row.id)
                    || row.state == connector::State::Connecting,
            )
            .ghost()
            .small()
            .icon(IconName::Ellipsis)
            .accessibility_label(tr("more"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let editor = owner.clone();
                let edit = row.clone();
                let reconnect = owner.clone();
                let login = row.clone();
                let probe = owner.clone();
                let probe_id = row.id;
                let disconnect = owner.clone();
                let disconnected = row.clone();
                let remove = owner.clone();
                let removed = row.clone();
                let menu = menu
                    .min_w(px(160.))
                    .item(
                        PopupMenuItem::new(tr("settings_edit")).on_click(move |_, window, cx| {
                            editor::open(editor.clone(), Some(edit.clone()), window, cx)
                        }),
                    )
                    .item(PopupMenuItem::new(tr("connector_reconnect")).on_click(
                        move |_, window, cx| {
                            editor::reconnect(reconnect.clone(), login.clone(), window, cx)
                        },
                    ))
                    .item(PopupMenuItem::new(tr("connector_probe")).on_click(
                        move |_, window, cx| {
                            probe.update(cx, |this, cx| {
                                this.connector_command(
                                    Command::ProbeConnection {
                                        connection: probe_id,
                                    },
                                    window,
                                    cx,
                                )
                            })
                        },
                    ));
                let menu = if row.state == connector::State::NeedsConfirmation {
                    let confirm = owner.clone();
                    let connection = row.id;
                    let expected_revision = row.revision;
                    let detail = consent::detail(&row);
                    menu.item(PopupMenuItem::new(tr("connector_confirmation")).on_click(
                        move |_, window, cx| {
                            let confirm = confirm.clone();
                            crate::prompts::confirm(
                                &tr("connector_confirmation"),
                                &detail,
                                tr("connector_confirm"),
                                window,
                                cx,
                                move |window, cx| {
                                    confirm.update(cx, |this, cx| {
                                        this.connector_command(
                                            Command::ConfirmConnectorConsent {
                                                connection,
                                                expected_revision,
                                                accept: true,
                                            },
                                            window,
                                            cx,
                                        )
                                    })
                                },
                            );
                        },
                    ))
                } else {
                    menu
                };
                menu.separator()
                    .item(PopupMenuItem::new(tr("connector_disconnect")).on_click(
                        move |_, window, cx| {
                            disconnect.update(cx, |this, cx| {
                                this.connector_command(
                                    Command::DisconnectConnection {
                                        connection: disconnected.id,
                                        expected_revision: disconnected.revision,
                                    },
                                    window,
                                    cx,
                                )
                            })
                        },
                    ))
                    .item(
                        PopupMenuItem::element(|_, cx| {
                            div()
                                .text_color(cx.theme().danger)
                                .child(tr("settings_delete"))
                        })
                        .on_click(move |_, window, cx| {
                            let remove = remove.clone();
                            let removed = removed.clone();
                            crate::prompts::confirm(
                                &tr("settings_delete"),
                                rust_i18n::t!(
                                    "connector_remove_effect",
                                    name = removed
                                        .identity
                                        .as_ref()
                                        .map(|identity| identity.name.as_str())
                                        .unwrap_or(&removed.connector)
                                )
                                .as_ref(),
                                tr("settings_delete"),
                                window,
                                cx,
                                move |window, cx| {
                                    remove.update(cx, |this, cx| {
                                        this.connector_command(
                                            Command::RemoveConnection {
                                                connection: removed.id,
                                                expected_revision: removed.revision,
                                            },
                                            window,
                                            cx,
                                        )
                                    })
                                },
                            );
                        }),
                    )
            })
            .into_any_element()
    }
}
