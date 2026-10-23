//! One set of row controls routes to the existing account and MCP command owners.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    native_menu::NativeMenu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Configure,
    Check,
    Remove,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_services, no_json)]
pub(in crate::settings::connectors) struct Dispatch {
    node: NodeId,
    key: catalog::Key,
    package: Reference,
    operation: Operation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Account(super::super::menu::Operation, &'static str, bool),
    Service(Operation, &'static str, bool),
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Self::Account(_, label, _) | Self::Service(_, label, _) => label,
        }
    }

    fn enabled(self) -> bool {
        match self {
            Self::Account(_, _, enabled) | Self::Service(_, _, enabled) => enabled,
        }
    }
}

fn items(service: &catalog::Service) -> Vec<Item> {
    if service.manual() {
        return vec![
            Item::Service(
                Operation::Configure,
                "connector_configure",
                service.info.is_some(),
            ),
            Item::Service(
                Operation::Check,
                "connector_probe",
                service
                    .servers
                    .iter()
                    .any(|server| checkable(service, server)),
            ),
            Item::Service(
                Operation::Remove,
                "connector_remove",
                service.info.is_some(),
            ),
        ];
    }
    let Some(account) = &service.account else {
        return Vec::new();
    };
    use super::super::menu::{Item as Entry, Operation as Account};
    let available = super::super::menu::items(account, service.kind.as_ref());
    [
        (Account::Edit, "connector_configure"),
        (Account::Probe, "connector_probe"),
        (Account::Remove, "connector_remove"),
    ]
    .into_iter()
    .map(|(operation, label)| {
        Item::Account(
            operation,
            label,
            available.contains(&Entry::Action(operation, label)),
        )
    })
    .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum Toggle {
    Account(connector::Id),
    Server(String),
}

fn switch_target(service: &catalog::Service) -> Option<Toggle> {
    match service.key.member {
        catalog::Member::Account(id) => service
            .account
            .as_ref()
            .filter(|account| account.id == id)
            .map(|account| Toggle::Account(account.id)),
        catalog::Member::Mcp => match service.servers.as_slice() {
            [server] => Some(Toggle::Server(server.name.clone())),
            _ => None,
        },
    }
}

pub(in crate::settings::connectors) fn controls(
    owner: Entity<Workspace>,
    details: Entity<Details>,
    service: &catalog::Service,
    online: bool,
    account_busy: bool,
    cx: &App,
) -> AnyElement {
    let state = details.read(cx);
    let node = state.binding.client.target();
    let id = format!(
        "connector-controls-{}-{:?}",
        service.key.package, service.key.member
    );
    let mut controls = h_flex().gap_2().items_center();
    let target = switch_target(service);
    if let Some(account) = service
        .account
        .as_ref()
        .filter(|account| target == Some(Toggle::Account(account.id)))
    {
        let command = Command::SetConnectionEnabled {
            connection: account.id,
            expected_revision: account.revision,
            enabled: !account.enabled,
        };
        let toggle_owner = owner.downgrade();
        controls = controls.child(
            Switch::new(format!("{id}-enabled"))
                .checked(account.enabled)
                .disabled(!online || account_busy)
                .accessibility_label(tr("connector_account_enabled"))
                .on_click(move |_, window, cx| {
                    _ = toggle_owner.update(cx, |owner, cx| {
                        if owner
                            .provider_link
                            .as_ref()
                            .is_some_and(|live| live.binding.client.target() == node)
                        {
                            owner.connector_command(command.clone(), window, cx);
                        }
                    });
                }),
        );
    } else if let Some(Toggle::Server(name)) = target {
        let server = service
            .servers
            .iter()
            .find(|server| server.name == name)
            .expect("declared switch target")
            .clone();
        let busy = state.pending.contains(&server.name);
        let uncertain = state.requests.contains_key(&server.name);
        let checked = if busy {
            state
                .drafts
                .get(&server.name)
                .copied()
                .unwrap_or_else(|| service.allowed(&server))
        } else {
            service.allowed(&server)
        };
        let toggle = details.clone();
        let recovery = details.clone();
        controls = controls
            .when(busy, |controls| controls.child(Spinner::new().small()))
            .child(
                Switch::new(format!("{id}-allowed"))
                    .checked(checked)
                    .disabled(!online || busy || uncertain)
                    .accessibility_label(tr("connector_server_allowed"))
                    .on_click(move |allowed, window, cx| {
                        toggle.update(cx, |details, cx| {
                            details.set_allowed(server.clone(), *allowed, window, cx)
                        });
                    }),
            )
            .when(uncertain && !busy, |controls| {
                controls.child(
                    crate::feedback::retry(format!("{id}-retry"))
                        .small()
                        .disabled(!online)
                        .on_click(move |_, window, cx| {
                            recovery.update(cx, |details, cx| details.retry(window, cx));
                        }),
                )
            });
    }
    let busy = account_busy || !state.pending.is_empty();
    let actions = items(service);
    let captured = service.clone();
    let menu_owner = owner.downgrade();
    controls
        .child(
            Button::new(format!("{id}-menu"))
                .ghost()
                .small()
                .icon(IconName::Ellipsis)
                .accessibility_label(tr("more"))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    actions
                        .iter()
                        .copied()
                        .fold(menu.min_w(px(160.)), |menu, entry| {
                            let service = captured.clone();
                            let owner = menu_owner.clone();
                            menu.item(
                                PopupMenuItem::new(tr(entry.label()))
                                    .disabled(!entry.enabled() || !online || busy)
                                    .on_click(move |_, window, cx| {
                                        if let Some(owner) = owner.upgrade() {
                                            dispatch(&owner, node, &service, entry, window, cx);
                                        }
                                    }),
                            )
                        })
                }),
        )
        .into_any_element()
}

fn dispatch(
    owner: &Entity<Workspace>,
    node: NodeId,
    service: &catalog::Service,
    entry: Item,
    window: &mut Window,
    cx: &mut App,
) {
    if !entry.enabled() {
        return;
    }
    match entry {
        Item::Account(operation, _, _) => {
            if let Some(account) = &service.account {
                let action = super::super::menu::Dispatch::new(account, operation, Some(node));
                owner.update(cx, |workspace, cx| {
                    workspace.connector_action(&action, window, cx)
                });
            }
        }
        Item::Service(operation, _, _) => {
            let action = Dispatch {
                node,
                key: service.key.clone(),
                package: service.package.clone(),
                operation,
            };
            owner.update(cx, |workspace, cx| {
                workspace.service_action(&action, window, cx)
            });
        }
    }
}

impl Workspace {
    pub(in crate::settings::connectors) fn service_context(
        &self,
        service: &catalog::Service,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !cfg!(any(target_os = "macos", target_os = "windows")) {
            return;
        }
        let Some(node) = self
            .connectors
            .binding
            .as_ref()
            .map(|binding| binding.client.target())
        else {
            return;
        };
        let online = self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.connected);
        let busy = service.account.as_ref().is_some_and(|account| {
            self.connectors.pending.contains(&account.id)
                || account.state == connector::State::Connecting
        });
        let mut menu = Menu::new("");
        for entry in items(service) {
            let item = match entry {
                Item::Account(operation, label, enabled) => {
                    let Some(account) = &service.account else {
                        continue;
                    };
                    MenuItem::action(
                        tr(label),
                        super::super::menu::Dispatch::new(account, operation, Some(node)),
                    )
                    .disabled(!enabled || !online || busy)
                }
                Item::Service(operation, label, enabled) => MenuItem::action(
                    tr(label),
                    Dispatch {
                        node,
                        key: service.key.clone(),
                        package: service.package.clone(),
                        operation,
                    },
                )
                .disabled(!enabled || !online || busy),
            };
            menu.items.push(item);
        }
        NativeMenu::from(menu).show(position, window, cx);
    }

    pub(in crate::settings::connectors) fn service_action(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .provider_link
            .as_ref()
            .is_none_or(|live| live.binding.client.target() != action.node)
            || self
                .connectors
                .binding
                .as_ref()
                .is_none_or(|binding| binding.client.target() != action.node)
        {
            failure(
                Fault::new(ErrorCode::RevisionConflict, "service target changed"),
                window,
                cx,
            );
            return;
        }
        let Some(service) = self
            .service_catalog(cx)
            .into_iter()
            .find(|service| service.key == action.key && service.package == action.package)
        else {
            failure(
                Fault::new(ErrorCode::RevisionConflict, "service changed"),
                window,
                cx,
            );
            return;
        };
        if !items(&service).iter().any(|entry| matches!(entry, Item::Service(operation, _, true) if *operation == action.operation)) { return }
        let details = self.service_controller(&service, window, cx);
        if !details.read(cx).pending.is_empty() {
            return;
        }
        let owner = cx.entity();
        let operation = action.operation;
        let node = action.node;
        // Form mounting may read Workspace; release its update lease first.
        window.defer(cx, move |window, cx| {
            if owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_none_or(|live| live.binding.client.target() != node)
            {
                return;
            }
            match operation {
                Operation::Configure => {
                    if let (Some(server), Some(info)) = (service.servers.first(), &service.info) {
                        details.update(cx, |details, cx| {
                            details.configure(info.clone(), server.name.clone(), window, cx)
                        });
                    }
                }
                Operation::Check => details.update(cx, |details, cx| {
                    for server in service
                        .servers
                        .iter()
                        .filter(|server| checkable(&service, server))
                    {
                        details.check(server.clone(), window, cx);
                    }
                }),
                Operation::Remove => {
                    if let Some(info) = service.info {
                        super::super::super::plugins::dialog::open(
                            owner,
                            Some(info.summary),
                            true,
                            window,
                            cx,
                        );
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
