//! Right-click uses the existing macOS/Windows adapter; Linux keeps the ellipsis menu.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Edit,
    Probe,
    Remove,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_connectors, no_json)]
pub(super) struct Dispatch {
    node: Option<sailry_protocol::NodeId>,
    connection: connector::Id,
    revision: u64,
    operation: Operation,
}

impl Dispatch {
    pub(super) fn new(
        row: &Connection,
        operation: Operation,
        node: Option<sailry_protocol::NodeId>,
    ) -> Self {
        Self {
            node,
            connection: row.id,
            revision: row.revision,
            operation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    Action(Operation, &'static str),
}

pub(super) fn checkable(row: &Connection) -> bool {
    row.enabled
        && matches!(
            row.state,
            connector::State::Connected | connector::State::Failed | connector::State::Expired
        )
}

pub(super) fn items(row: &Connection, kind: Option<&Type>) -> Vec<Item> {
    let mut entries = Vec::new();
    if kind.is_some() {
        entries.push(Item::Action(Operation::Edit, "connector_configure"));
        if checkable(row) && kind.is_some_and(|kind| kind.enabled) {
            entries.push(Item::Action(Operation::Probe, "connector_probe"));
        }
    }
    entries.push(Item::Action(Operation::Remove, "connector_remove"));
    entries
}

impl Workspace {
    fn connector_items(&self, row: &Connection) -> Vec<Item> {
        let kind = self.connectors.types.iter().find(|kind| {
            kind.package.name == row.package.name && kind.declaration.id == row.connector
        });
        items(row, kind)
    }

    pub(super) fn connector_action(
        &mut self,
        dispatch: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if dispatch.node.is_none()
            || dispatch.node
                != self
                    .provider_link
                    .as_ref()
                    .map(|live| live.binding.client.target())
            || dispatch.node
                != self
                    .connectors
                    .binding
                    .as_ref()
                    .map(|binding| binding.client.target())
        {
            failure(
                Fault::new(ErrorCode::RevisionConflict, "connection target changed"),
                window,
                cx,
            );
            return;
        }
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
            Operation::Probe => Command::ProbeConnection { connection: row.id },
            Operation::Remove => Command::RemoveConnection {
                connection: row.id,
                expected_revision: row.revision,
            },
        };
        let confirmation = match dispatch.operation {
            Operation::Remove => Some((
                "connector_remove",
                "connector_remove",
                rust_i18n::t!("connector_remove_effect", name = name).to_string(),
            )),
            _ => None,
        };
        let node = dispatch.node;
        if let Some((title, button, detail)) = confirmation {
            crate::prompts::confirm(
                &tr(title),
                &detail,
                tr(button),
                window,
                cx,
                move |window, cx| {
                    owner.update(cx, |this, cx| {
                        if node.is_some()
                            && this
                                .provider_link
                                .as_ref()
                                .map(|live| live.binding.client.target())
                                == node
                            && this
                                .connectors
                                .binding
                                .as_ref()
                                .map(|binding| binding.client.target())
                                == node
                        {
                            this.connector_command(command.clone(), window, cx);
                        } else {
                            failure(
                                Fault::new(
                                    ErrorCode::RevisionConflict,
                                    "connection target changed",
                                ),
                                window,
                                cx,
                            );
                        }
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
