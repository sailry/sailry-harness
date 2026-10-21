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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Action(Operation, &'static str),
    Separator,
}

pub(super) fn checkable(row: &Connection) -> bool {
    row.enabled
        && matches!(
            row.state,
            connector::State::Connected | connector::State::Failed | connector::State::Expired
        )
}

fn items(row: &Connection, kind: Option<&Type>) -> Vec<Item> {
    if row.state == connector::State::Connecting {
        return Vec::new();
    }
    let mut items = Vec::new();
    let method = kind.and_then(|kind| {
        kind.declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == row.method)
    });
    let confirming = row.state == connector::State::NeedsConfirmation;
    if confirming {
        items.push(Item::Action(Operation::Confirm, "connector_confirm"));
    }
    if method.is_some()
        && kind.is_some_and(|kind| {
            kind.declaration
                .fields
                .iter()
                .any(|field| editable(field, row.method))
        })
    {
        items.push(Item::Action(Operation::Edit, "connector_configure"));
    }
    if !confirming && let Some(method) = method {
        match method {
            connector::Auth::Oauth2 { .. } | connector::Auth::McpOauth { .. } => {
                items.push(Item::Action(Operation::Reconnect, "connector_authorize"));
            }
            connector::Auth::LocalCli { .. } => {
                items.push(Item::Action(Operation::Reconnect, "connector_bind"));
            }
            _ => (),
        }
        if checkable(row) {
            items.push(Item::Action(Operation::Probe, "connector_probe"));
        }
    }
    if !items.is_empty() {
        items.push(Item::Separator);
    }
    if row.can_disconnect {
        items.push(Item::Action(Operation::Disconnect, "connector_disconnect"));
    }
    items.push(Item::Action(Operation::Remove, "settings_delete"));
    items
}

impl Workspace {
    fn connector_items(&self, row: &Connection) -> Vec<Item> {
        let kind = self.connectors.types.iter().find(|kind| {
            kind.package.name == row.package.name && kind.declaration.id == row.connector
        });
        items(row, kind)
    }

    pub(super) fn connector_menu(&self, row: &Connection, cx: &mut Context<Self>) -> AnyElement {
        let actions = self.connector_items(row);
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
                actions
                    .iter()
                    .copied()
                    .fold(menu.min_w(px(160.)), |menu, item| {
                        let Item::Action(operation, key) = item else {
                            return menu.separator();
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
        for item in self.connector_items(row) {
            let Item::Action(operation, key) = item else {
                menu.items.push(MenuItem::Separator);
                continue;
            };
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
        if !self.connector_items(&row).iter().any(
            |item| matches!(item, Item::Action(operation, _) if *operation == dispatch.operation),
        ) {
            return;
        }
        let name = row
            .identity
            .as_ref()
            .map(|identity| identity.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| {
                self.connectors
                    .types
                    .iter()
                    .find(|kind| {
                        kind.package.name == row.package.name
                            && kind.declaration.id == row.connector
                    })
                    .map(|kind| title(&kind.declaration.display))
            })
            .unwrap_or_else(|| row.connector.clone());
        let owner = cx.entity();
        let command = match dispatch.operation {
            // The editor reads its workspace, which must be released before mounting it.
            Operation::Edit => {
                return window.defer(cx, move |window, cx| {
                    editor::open(owner, Some(row), window, cx)
                });
            }
            Operation::Reconnect => {
                return window.defer(cx, move |window, cx| {
                    editor::reconnect(owner, row, window, cx)
                });
            }
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
            Operation::Disconnect => Some((
                "connector_disconnect",
                "connector_disconnect",
                rust_i18n::t!("connector_disconnect_effect", name = name).to_string(),
            )),
            Operation::Remove => Some((
                "settings_delete",
                "settings_delete",
                rust_i18n::t!("connector_remove_effect", name = name).to_string(),
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

#[cfg(test)]
mod tests;
