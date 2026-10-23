//! List controls reuse the account and server command owners.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    native_menu::NativeMenu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Configure,
    Tools,
    Check,
    Connect,
    Plugin,
    Remove,
    Enable,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_services, no_json)]
pub(in crate::settings::connectors) struct Dispatch {
    node: NodeId,
    key: catalog::Key,
    package: Reference,
    operation: Operation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Account(super::super::menu::Operation, &'static str),
    Service(Operation, &'static str, bool),
    Separator,
}

fn items(service: &catalog::Service) -> Vec<Item> {
    let mut entries = Vec::new();
    if !service.servers.is_empty() {
        entries.push(Item::Service(
            Operation::Configure,
            "connector_configure",
            service.info.is_some(),
        ));
        if let [server] = service.servers.as_slice() {
            entries.push(Item::Service(
                Operation::Check,
                "connector_probe",
                view::checkable(service, server),
            ));
        }
    }
    if let Some(account) = &service.account {
        for entry in super::super::menu::items(account, service.kind.as_ref()) {
            match entry {
                super::super::menu::Item::Action(operation, label) => {
                    if !service.servers.is_empty()
                        && matches!(
                            operation,
                            super::super::menu::Operation::Edit
                                | super::super::menu::Operation::Probe
                        )
                    {
                        continue;
                    }
                    entries.push(Item::Account(operation, label));
                }
                super::super::menu::Item::Separator if !entries.is_empty() => {
                    entries.push(Item::Separator)
                }
                _ => (),
            }
        }
    } else if service.kind.is_some() {
        entries.push(Item::Service(
            Operation::Connect,
            "connector_connect",
            service.enabled(),
        ));
    }
    if service.info.is_some() {
        if !entries.is_empty() {
            entries.push(Item::Separator);
        }
        entries.push(Item::Service(Operation::Tools, "plugins_information", true));
        if service
            .info
            .as_ref()
            .is_some_and(|info| info.mcp_source.is_some())
        {
            if service
                .info
                .as_ref()
                .is_some_and(|info| !info.summary.enabled)
            {
                entries.push(Item::Service(Operation::Enable, "connector_enable", true));
            }
            entries.push(Item::Service(Operation::Remove, "plugins_uninstall", true));
        } else {
            entries.push(Item::Service(
                Operation::Plugin,
                "connector_view_plugin",
                true,
            ));
        }
    }
    entries
}

#[derive(Debug, PartialEq, Eq)]
enum Toggle {
    Account(connector::Id),
    Server(String),
}

fn switch_target(service: &catalog::Service) -> Option<Toggle> {
    service.info.as_ref()?;
    let manual = service
        .info
        .as_ref()
        .is_some_and(|info| info.mcp_source.is_some());
    let server = match &service.key.member {
        catalog::Member::Server(name) => service.servers.iter().find(|server| server.name == *name),
        catalog::Member::Account(_) if manual => {
            let kind = service.kind.as_ref()?;
            let bound = service
                .servers
                .iter()
                .filter(|server| {
                    crate::plugins::metadata::binds_server(&kind.declaration, &server.name)
                })
                .collect::<Vec<_>>();
            match bound.as_slice() {
                [server] => Some(*server),
                _ => None,
            }
        }
        catalog::Member::Account(_) => {
            return service
                .account
                .as_ref()
                .map(|account| Toggle::Account(account.id));
        }
    };
    server.map(|server| Toggle::Server(server.name.clone()))
}

pub(in crate::settings::connectors) fn controls(
    owner: Entity<Workspace>,
    details: Entity<Details>,
    service: &catalog::Service,
    online: bool,
    account_busy: bool,
    package_busy: bool,
    package_retry: bool,
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
        let checked = state
            .drafts
            .get(&server.name)
            .copied()
            .unwrap_or(server.allowed);
        let toggle = details.clone();
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
            );
    } else {
        controls = controls.child(
            Switch::new(format!("{id}-enabled"))
                .checked(false)
                .disabled(true)
                .accessibility_label(tr("connector_connect")),
        );
    }
    let single_busy = match service.servers.as_slice() {
        [server] => state.pending.contains(&server.name),
        _ => false,
    };
    let actions = items(service)
        .into_iter()
        .map(|entry| match entry {
            Item::Service(Operation::Enable, label, enabled) => Item::Service(
                Operation::Enable,
                if package_retry {
                    "plugins_retry"
                } else {
                    label
                },
                enabled && (!package_busy || package_retry),
            ),
            Item::Service(operation, label, enabled)
                if matches!(operation, Operation::Check | Operation::Configure)
                    && service.servers.len() == 1 =>
            {
                Item::Service(operation, label, enabled && !single_busy)
            }
            other => other,
        })
        .collect::<Vec<_>>();
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
                    actions.iter().fold(menu.min_w(px(160.)), |menu, entry| {
                        let (label, enabled) = match *entry {
                            Item::Account(_, label) => (label, online && !account_busy),
                            Item::Service(_, label, enabled) => (label, enabled && online),
                            Item::Separator => return menu.separator(),
                        };
                        let entry = *entry;
                        let service = captured.clone();
                        let owner = menu_owner.clone();
                        menu.item(PopupMenuItem::new(tr(label)).disabled(!enabled).on_click(
                            move |_, window, cx| {
                                if let Some(owner) = owner.upgrade() {
                                    dispatch(&owner, node, &service, entry, window, cx);
                                }
                            },
                        ))
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
    match entry {
        Item::Account(operation, _) => {
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
        Item::Separator => (),
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
        let mut menu = Menu::new("");
        for entry in items(service) {
            let item = match entry {
                Item::Account(operation, label) => {
                    let Some(account) = &service.account else {
                        continue;
                    };
                    MenuItem::action(
                        tr(label),
                        super::super::menu::Dispatch::new(account, operation, Some(node)),
                    )
                }
                Item::Service(operation, label, enabled) => MenuItem::action(
                    tr(
                        if operation == Operation::Enable
                            && self
                                .plugin_catalog
                                .retries()
                                .iter()
                                .any(|name| name == &service.package.name)
                        {
                            "plugins_retry"
                        } else {
                            label
                        },
                    ),
                    Dispatch {
                        node,
                        key: service.key.clone(),
                        package: service.package.clone(),
                        operation,
                    },
                )
                .disabled(
                    !enabled
                        || (operation == Operation::Enable
                            && self.plugin_catalog.busy_package(&service.package.name, cx)
                            && !self
                                .plugin_catalog
                                .retries()
                                .iter()
                                .any(|name| name == &service.package.name)),
                ),
                Item::Separator => MenuItem::Separator,
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
        if !items(&service).iter().any(|entry| matches!(entry, Item::Service(operation, _, true) if *operation == action.operation)) { return; }
        if action.operation == Operation::Enable {
            if self
                .plugin_catalog
                .retries()
                .iter()
                .any(|name| name == &service.package.name)
            {
                self.send_plugin_action(&service.package.name, window, cx);
            } else if let Some(info) = &service.info {
                self.toggle_plugin(&info.summary, true, window, cx);
            }
            return;
        }
        let details = self.service_controller(&service, window, cx);
        let owner = cx.entity();
        let operation = action.operation;
        // Form mounting may read Workspace; release its update lease first.
        let node = action.node;
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
                    if let ([server], Some(info)) = (service.servers.as_slice(), &service.info) {
                        details.update(cx, |details, cx| {
                            details.configure(info.clone(), server.name.clone(), window, cx)
                        });
                    } else {
                        view::open(details, window, cx);
                    }
                }
                Operation::Tools => view::open(details, window, cx),
                Operation::Check => {
                    if let [server] = service.servers.as_slice() {
                        details.update(cx, |details, cx| details.check(server.clone(), window, cx));
                    }
                }
                Operation::Connect => {
                    if let Some(kind) = service.kind {
                        editor::connect(owner, kind, window, cx);
                    }
                }
                Operation::Plugin => window.dispatch_action(
                    Box::new(crate::settings::OpenManagement {
                        node: details.read(cx).binding.client.target(),
                        name: service.package.name,
                        skill: None,
                    }),
                    cx,
                ),
                Operation::Enable => (),
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
