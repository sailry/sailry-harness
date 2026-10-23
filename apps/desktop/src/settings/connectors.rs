//! Connection presentation uses the same Client contracts for local and remote Nodes.
mod catalog;
mod commands;
mod consent;
mod details;
mod editor;
pub(in crate::settings) use editor::manage;
mod filter;
mod menu;
use super::{Workspace, providers::Binding};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        select::{Select, SelectEvent, SelectState},
        spinner::Spinner,
        switch::Switch,
        tag::Tag,
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, Request, Update,
    plugin::connector::{self, Connection, Type},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct State {
    query: Entity<InputState>,
    method: Entity<SelectState<Vec<filter::Method>>>,
    focus: FocusHandle,
    types: Vec<Type>,
    rows: Vec<Connection>,
    binding: Option<Binding>,
    loading: bool,
    received: bool,
    failed: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
    pending: BTreeSet<connector::Id>,
    requests: BTreeMap<connector::Id, commands::Submission>,
    services: BTreeMap<catalog::Key, Entity<details::Details>>,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("connector_search")));
        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        let method = cx.new(|cx| {
            SelectState::new(
                filter::Method::ALL.to_vec(),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        cx.subscribe(&method, |_, _, event, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                cx.notify();
            }
        })
        .detach();
        Self {
            query,
            method,
            focus: cx.focus_handle(),
            types: Vec::new(),
            rows: Vec::new(),
            binding: None,
            loading: false,
            received: false,
            failed: false,
            stop: CancellationToken::new(),
            task: None,
            pending: BTreeSet::new(),
            requests: BTreeMap::new(),
            services: BTreeMap::new(),
        }
    }
    pub(super) fn close(&mut self) {
        self.stop.cancel();
        self.task = None;
        self.loading = false;
    }

    pub(super) fn heading(&self) -> SharedString {
        tr("settings_connectors")
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

enum Message {
    Rows(connector::Changes),
    Failed(Fault),
}

pub(in crate::settings) fn open_management(
    owner: Entity<Workspace>,
    info: sailry_protocol::plugin::Info,
    window: &mut Window,
    cx: &mut App,
) {
    owner.update(cx, |owner, cx| owner.warm_connectors(window, cx));
    if info.mcp_source.is_some() {
        if let (Some(binding), Some(server)) = (
            owner.read(cx).connectors.binding.clone(),
            info.mcp.first().map(|server| server.name.clone()),
        ) {
            super::mcp::configure_server(owner, binding, info, server, window, cx);
        }
    } else {
        editor::manage(owner, info.summary.reference(), window, cx);
    }
}

fn availability(value: sailry_protocol::plugin::mcp::Availability) -> Option<&'static str> {
    use sailry_protocol::plugin::mcp::Availability;
    match value {
        Availability::Available => None,
        Availability::PackageDisabled => Some("connector_package_disabled"),
        Availability::ServerDisabled => Some("connector_server_disabled"),
        Availability::ConfigurationRequired => Some("connector_configuration_required"),
        Availability::ConnectionRequired => Some("connector_not_connected"),
        Availability::ConnectionDisabled => Some("connector_account_disabled"),
        Availability::AuthorizationExpired => Some("connector_expired"),
        Availability::ConsentRequired => Some("connector_confirmation"),
        Availability::Unavailable => Some("connector_unavailable"),
    }
}

fn status(service: &catalog::Service) -> Option<&'static str> {
    if !service.enabled() {
        return Some("connector_package_disabled");
    }
    if service.manual() {
        return service
            .servers
            .iter()
            .find_map(|server| availability(server.availability));
    }
    if let Some(account) = &service.account {
        if !account.enabled {
            return Some("connector_account_disabled");
        }
        match account.state {
            connector::State::NotConnected => return Some("connector_not_connected"),
            connector::State::Expired => return Some("connector_expired"),
            connector::State::NeedsConfirmation => return Some("connector_confirmation"),
            connector::State::Connecting => return Some("provider_login_waiting"),
            connector::State::Failed => return Some("connector_failed"),
            _ => (),
        }
    }
    service
        .servers
        .iter()
        .find_map(|server| availability(server.availability))
        .or_else(|| {
            (service.servers.is_empty() && service.account.is_none())
                .then_some("connector_not_connected")
        })
}

impl Workspace {
    pub(super) fn warm_connectors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.provider_link.as_ref().map(|live| live.binding.clone()) else {
            return;
        };
        if self
            .connectors
            .binding
            .as_ref()
            .is_some_and(|previous| previous.client.target() == binding.client.target())
            && self.connectors.task.is_some()
        {
            return;
        }
        self.connectors.close();
        self.connectors.stop = CancellationToken::new();
        self.connectors.loading = true;
        self.connectors.failed = false;
        if self
            .connectors
            .binding
            .as_ref()
            .is_none_or(|previous| previous.client.target() != binding.client.target())
        {
            self.connectors.rows.clear();
            self.connectors.types.clear();
            self.connectors.requests.clear();
            self.connectors.pending.clear();
            self.connectors.services.clear();
            self.connectors.received = false;
        }
        self.connectors.binding = Some(binding.clone());
        let node = binding.client.target();
        let stop = self.connectors.stop.clone();
        let observed = stop.clone();
        let client = binding.client.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job=binding.runtime.spawn(async move{
            let operation=async{
                let mut subscription=client.subscribe_connections().await?;
                loop{
                    let update=tokio::select!{biased;_=stop.cancelled()=>return Ok(()),update=subscription.next()=>update?};
                    match update{Update::ConnectionsChanged(changes)=>{let _=sender.send(Message::Rows(changes));},Update::ResetRequired=>return Err(Fault::new(ErrorCode::Unavailable,"connector observation interrupted")),_=>return Err(Fault::invalid("connector update expected"))}
                }
            };
            if let Err(error)=operation.await{let _=sender.send(Message::Failed(error));}
        });
        self.connectors.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let _job = job;
            while let Some(message) = receiver.recv().await {
                if owner
                    .update_in(cx, |this, window, cx| {
                        if observed.is_cancelled() {
                            return;
                        }
                        match message {
                            Message::Rows(changes) => {
                                let changed = catalog::changed_accounts(
                                    &this.connectors.rows,
                                    &changes.connections,
                                );
                                if !changed.is_empty()
                                    && this
                                        .provider_link
                                        .as_ref()
                                        .is_some_and(|live| live.binding.client.target() == node)
                                    && let Some(metadata) = &this.plugin_catalog.metadata
                                {
                                    metadata.update(cx, |metadata, cx| {
                                        metadata.invalidate(&changed, cx)
                                    });
                                }
                                this.connectors.rows = changes.connections;
                                this.connectors.types = changes.types;
                                commands::reconcile(
                                    &mut this.connectors.requests,
                                    &this.connectors.pending,
                                    &this.connectors.rows,
                                );
                                this.connectors.loading = false;
                                this.connectors.received = true;
                                this.connectors.failed = false;
                            }
                            Message::Failed(error) => {
                                this.connectors.loading = false;
                                this.connectors.failed = true;
                                failure(error, window, cx);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(super) fn connectors(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.warm_connectors(window, cx);
        let query = self.connectors.query.read(cx).value().to_lowercase();
        let method = self
            .connectors
            .method
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or_default();
        let catalog = self.service_catalog(cx);
        let keys = catalog
            .iter()
            .map(|service| service.key.clone())
            .collect::<BTreeSet<_>>();
        self.connectors.services.retain(|key, _| {
            keys.contains(key)
                || (key.member == catalog::Member::Mcp
                    && self
                        .plugin_catalog
                        .packages
                        .iter()
                        .any(|package| package.name == key.package))
        });
        let services = catalog
            .into_iter()
            .filter(|service| service.matches(method, &query))
            .collect::<Vec<_>>();
        let owner = cx.entity();
        let disabled = self
            .provider_link
            .as_ref()
            .is_none_or(|live| !live.connected);
        let preview = self.connectors.binding.is_none();
        let loading = !self.connectors.received
            || self.connectors.failed
            || self.connectors.loading
            || self
                .provider_link
                .as_ref()
                .is_none_or(|live| live.snapshot.is_none())
            || self
                .plugin_catalog
                .metadata
                .as_ref()
                .is_none_or(|metadata| {
                    let metadata = metadata.read(cx);
                    !metadata.settled() || !metadata.errors.is_empty()
                });
        let metadata_failed = self
            .plugin_catalog
            .metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.read(cx).errors.is_empty());
        let mcp_binding = self.connectors.binding.clone();
        let mcp_owner = owner.clone();
        let toolbar = h_flex()
            .w_full()
            .gap_3()
            .child(
                Input::new(&self.connectors.query)
                    .prefix(IconName::Search)
                    .cleanable(true)
                    .flex_1(),
            )
            .child(div().w_48().child(
                Select::new(&self.connectors.method).accessibility_label(tr("connector_filter")),
            ))
            .child(
                Button::new("connector-add")
                    .primary()
                    .label(tr("settings_add"))
                    .disabled(disabled)
                    .on_click(move |_, window, cx| editor::open(owner.clone(), None, window, cx)),
            )
            .when(metadata_failed || self.connectors.failed, |toolbar| {
                toolbar.child(
                    crate::feedback::retry("connectors-metadata-retry").on_click(cx.listener(
                        |workspace, _, window, cx| {
                            if workspace.connectors.failed {
                                workspace.connectors.close();
                                workspace.warm_connectors(window, cx);
                            }
                            if let Some(metadata) = &workspace.plugin_catalog.metadata {
                                metadata.update(cx, |metadata, cx| metadata.refresh(cx));
                            }
                        },
                    )),
                )
            })
            .child(
                Button::new("connector-add-mcp")
                    .label(tr("mcp_add"))
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        if let Some(binding) = &mcp_binding {
                            super::mcp::add(mcp_owner.clone(), binding.clone(), window, cx);
                        }
                    }),
            );
        let rows = services
            .iter()
            .map(|service| self.service_row(service, window, cx))
            .collect::<Vec<_>>();
        let content = if services.is_empty() && loading {
            div().into_any_element()
        } else {
            super::group::Group::new("settings_connectors")
                .heading(false)
                .empty(IconName::Network, "connector_empty")
                .children(rows)
                .into_any_element()
        };
        v_flex()
            .track_focus(&self.connectors.focus)
            .on_action(cx.listener(Self::connector_action))
            .on_action(cx.listener(Self::service_action))
            .w_full()
            .gap_4()
            .child(toolbar)
            .when(preview, |element| {
                element.child(Tag::new().child(tr("connector_preview")))
            })
            .child(content)
            .into_any_element()
    }

    fn service_catalog(&self, cx: &App) -> Vec<catalog::Service> {
        let metadata = self
            .plugin_catalog
            .metadata
            .as_ref()
            .map(|metadata| {
                let metadata = metadata.read(cx);
                metadata
                    .entries
                    .iter()
                    .filter(|(_, info)| {
                        self.plugin_catalog
                            .packages
                            .iter()
                            .any(|summary| *summary == info.summary)
                    })
                    .map(|(name, info)| (name.clone(), info.clone()))
                    .collect()
            })
            .unwrap_or_default();
        catalog::collect(&self.connectors.types, &self.connectors.rows, &metadata)
    }

    fn service_controller(
        &mut self,
        service: &catalog::Service,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<details::Details> {
        if let Some(controller) = self.connectors.services.get(&service.key) {
            return controller.clone();
        }
        let binding = self
            .connectors
            .binding
            .clone()
            .expect("service inventory is bound");
        let controller = details::create(cx.entity(), binding, service.key.clone(), window, cx);
        self.connectors
            .services
            .insert(service.key.clone(), controller.clone());
        controller
    }

    fn service_row(
        &mut self,
        service: &catalog::Service,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = service.label();
        let status = status(service);
        let description = service
            .account
            .as_ref()
            .and_then(|account| account.identity.as_ref())
            .map(|identity| identity.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| {
                service
                    .info
                    .as_ref()
                    .and_then(crate::plugins::metadata::description)
            })
            .unwrap_or_else(|| service.package.name.clone());
        let icon = service
            .kind
            .as_ref()
            .and_then(|kind| kind.declaration.display.icon.as_ref())
            .or_else(|| {
                service
                    .info
                    .as_ref()
                    .and_then(crate::plugins::emblem::glyph)
            })
            .map(crate::assets::icons::icon)
            .unwrap_or_else(|| Icon::new(gpui_kit::assets::IconName::Plug));
        let key = service.key.clone();
        let owner = cx.entity();
        let id = format!("connector-service-{}-{:?}", key.package, key.member);
        let entry = super::entry::Entry::new(
            id.clone(),
            h_flex()
                .items_center()
                .gap_3()
                .child(crate::plugins::emblem::surface(&label, None, px(36.)).child(icon.size_5()))
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().truncate().child(label.clone()))
                                .when_some(status, |row, key| {
                                    row.child(Tag::new().child(tr(key)).xsmall())
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                ),
        )
        .control(details::controls(
            owner,
            self.service_controller(service, window, cx),
            service,
            self.provider_link
                .as_ref()
                .is_some_and(|live| live.connected),
            service.account.as_ref().is_some_and(|account| {
                self.connectors.pending.contains(&account.id)
                    || account.state == connector::State::Connecting
            }),
            cx,
        ));
        let context = service.clone();
        div()
            .id(SharedString::from(id))
            .child(entry)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.connectors.focus.focus(window, cx);
                    this.service_context(&context, event.position, window, cx);
                }),
            )
            .into_any_element()
    }
}

fn editable(field: &connector::Field, method: connector::Kind) -> bool {
    method != connector::Kind::LocalCli || !field.secret
}

fn title(navigation: &sailry_protocol::plugin::desktop::Navigation) -> String {
    navigation
        .locales
        .get::<str>(rust_i18n::locale().as_ref())
        .cloned()
        .unwrap_or_else(|| navigation.label.clone())
}
fn failure(error: Fault, window: &mut Window, cx: &mut App) {
    let key = match error.code {
        ErrorCode::RevisionConflict => "connector_conflict",
        ErrorCode::OutcomeUnknown => "connector_unknown",
        ErrorCode::Expired => "connector_expired",
        _ => "connector_failed",
    };
    crate::feedback::error("", &tr(key), window, cx);
}
