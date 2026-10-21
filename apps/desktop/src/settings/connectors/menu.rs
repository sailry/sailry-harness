//! Right-click uses the existing macOS/Windows adapter; Linux keeps the ellipsis menu.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    native_menu::NativeMenu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Edit,
    Reconnect,
    Probe,
    Confirm,
    Disconnect,
    Remove,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_connectors, no_json)]
pub(super) struct Dispatch {
    connection: connector::Id,
    revision: u64,
    operation: Operation,
}

fn items(row: &Connection) -> Vec<(Operation, &'static str)> {
    let mut items = vec![
        (Operation::Edit, "settings_edit"),
        (Operation::Reconnect, "connector_reconnect"),
        (Operation::Probe, "connector_probe"),
    ];
    if row.state == connector::State::NeedsConfirmation {
        items.push((Operation::Confirm, "connector_confirmation"));
    }
    items.extend([
        (Operation::Disconnect, "connector_disconnect"),
        (Operation::Remove, "settings_delete"),
    ]);
    items
}

impl Workspace {
    pub(super) fn connector_menu(&self, row: &Connection, cx: &mut Context<Self>) -> AnyElement {
        let row = row.clone();
        let owner = cx.entity();
        Button::new(format!("connector-menu-{}", row.id))
            .disabled(
                self.connectors.pending.contains(&row.id)
                    || row.state == connector::State::Connecting,
            )
            .ghost()
            .small()
            .icon(IconName::Ellipsis)
            .accessibility_label(tr("more"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                items(&row)
                    .into_iter()
                    .fold(menu.min_w(px(160.)), |menu, (operation, key)| {
                        let menu = if operation == Operation::Disconnect {
                            menu.separator()
                        } else {
                            menu
                        };
                        let item = if operation == Operation::Remove {
                            PopupMenuItem::element(move |_, cx| {
                                div().text_color(cx.theme().danger).child(tr(key))
                            })
                        } else {
                            PopupMenuItem::new(tr(key))
                        };
                        let owner = owner.clone();
                        let dispatch = Dispatch {
                            connection: row.id,
                            revision: row.revision,
                            operation,
                        };
                        menu.item(item.on_click(move |_, window, cx| {
                            owner.update(cx, |this, cx| {
                                this.connector_action(&dispatch, window, cx)
                            });
                        }))
                    })
            })
            .into_any_element()
    }

    pub(super) fn connector_context(
        &self,
        row: &Connection,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.connectors.pending.contains(&row.id) || row.state == connector::State::Connecting {
            return;
        }
        if !cfg!(any(target_os = "macos", target_os = "windows")) {
            return;
        }
        let mut menu = Menu::new("");
        for (operation, key) in items(row) {
            if operation == Operation::Disconnect {
                menu.items.push(MenuItem::Separator);
            }
            menu.items.push(MenuItem::action(
                tr(key),
                Dispatch {
                    connection: row.id,
                    revision: row.revision,
                    operation,
                },
            ));
        }
        NativeMenu::from(menu).show(position, window, cx);
    }

    pub(super) fn connector_action(
        &mut self,
        dispatch: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self
            .connectors
            .rows
            .iter()
            .find(|row| row.id == dispatch.connection)
            .cloned()
        else {
            failure(
                Fault::new(ErrorCode::RevisionConflict, "connection changed"),
                window,
                cx,
            );
            return;
        };
        if row.revision != dispatch.revision {
            failure(
                Fault::new(ErrorCode::RevisionConflict, "connection changed"),
                window,
                cx,
            );
            return;
        }
        if self.connectors.pending.contains(&row.id) || row.state == connector::State::Connecting {
            return;
        }
        let owner = cx.entity();
        let command = match dispatch.operation {
            Operation::Edit => return editor::open(owner, Some(row), window, cx),
            Operation::Reconnect => return editor::reconnect(owner, row, window, cx),
            Operation::Probe => Command::ProbeConnection { connection: row.id },
            Operation::Confirm => Command::ConfirmConnectorConsent {
                connection: row.id,
                expected_revision: row.revision,
                accept: true,
            },
            Operation::Disconnect => Command::DisconnectConnection {
                connection: row.id,
                expected_revision: row.revision,
            },
            Operation::Remove => Command::RemoveConnection {
                connection: row.id,
                expected_revision: row.revision,
            },
        };
        let confirmation = match dispatch.operation {
            Operation::Confirm => Some((
                "connector_confirmation",
                "connector_confirm",
                consent::detail(&row),
            )),
            Operation::Remove => Some((
                "settings_delete",
                "settings_delete",
                rust_i18n::t!(
                    "connector_remove_effect",
                    name = row
                        .identity
                        .as_ref()
                        .map(|identity| identity.name.as_str())
                        .unwrap_or(&row.connector),
                )
                .to_string(),
            )),
            _ => None,
        };
        if let Some((title, button, detail)) = confirmation {
            crate::prompts::confirm(
                &tr(title),
                &detail,
                tr(button),
                window,
                cx,
                move |window, cx| {
                    owner.update(cx, |this, cx| {
                        this.connector_command(command.clone(), window, cx)
                    });
                },
            );
        } else {
            self.connector_command(command, window, cx);
        }
    }
}
