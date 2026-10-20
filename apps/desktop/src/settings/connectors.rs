//! Connection presentation uses the same Client contracts for local and remote Nodes.
mod consent;
mod editor;
mod menu;
use super::{Workspace, providers::Binding};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        skeleton::Skeleton,
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
    types: Vec<Type>,
    rows: Vec<Connection>,
    binding: Option<Binding>,
    loading: bool,
    refreshing: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
    pending: BTreeSet<connector::Id>,
    requests: BTreeMap<connector::Id, Request>,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("connector_search")));
        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        Self {
            query,
            types: Vec::new(),
            rows: Vec::new(),
            binding: None,
            loading: false,
            refreshing: false,
            stop: CancellationToken::new(),
            task: None,
            pending: BTreeSet::new(),
            requests: BTreeMap::new(),
        }
    }
    pub(super) fn close(&mut self) {
        self.stop.cancel();
        self.task = None;
        self.loading = false;
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
        }
        self.connectors.binding = Some(binding.clone());
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
                                this.connectors.rows = changes.connections;
                                this.connectors.types = changes.types;
                                this.connectors.loading = false;
                            }
                            Message::Failed(error) => {
                                this.connectors.loading = false;
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
        let rows = self
            .connectors
            .rows
            .iter()
            .filter(|row| {
                let label = self
                    .connectors
                    .types
                    .iter()
                    .find(|kind| {
                        kind.package.name == row.package.name
                            && kind.declaration.id == row.connector
                    })
                    .map(|kind| title(&kind.declaration.display));
                format!(
                    "{} {} {}",
                    label.unwrap_or_else(|| row.connector.clone()),
                    row.identity
                        .as_ref()
                        .map(|identity| identity.name.as_str())
                        .unwrap_or_default(),
                    row.package.name
                )
                .to_lowercase()
                .contains(&query)
            })
            .cloned()
            .collect::<Vec<_>>();
        let owner = cx.entity();
        let disabled = self.connectors.binding.is_none();
        let toolbar = h_flex()
            .w_full()
            .gap_3()
            .child(Input::new(&self.connectors.query).cleanable(true).flex_1())
            .child(
                crate::feedback::retry("connector-refresh")
                    .disabled(disabled || self.connectors.loading || self.connectors.refreshing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.refresh_connectors(window, cx);
                    })),
            )
            .child(
                Button::new("connector-add")
                    .primary()
                    .label(tr("settings_add"))
                    .disabled(disabled)
                    .on_click(move |_, window, cx| editor::open(owner.clone(), None, window, cx)),
            );
        let content = super::group::Group::new("settings_connectors")
            .heading(false)
            .empty(IconName::Network, "connector_empty")
            .children(rows.iter().map(|row| self.connector_row(row, cx)));
        v_flex()
            .w_full()
            .gap_4()
            .child(toolbar)
            .when(disabled, |element| {
                element.child(Tag::new().child(tr("connector_preview")))
            })
            .when(self.connectors.loading, |element| {
                element.child(Skeleton::new().h_12().w_full())
            })
            .child(content)
            .into_any_element()
    }

    fn connector_row(&self, row: &Connection, cx: &mut Context<Self>) -> AnyElement {
        let declaration = self
            .connectors
            .types
            .iter()
            .find(|kind| {
                kind.package.name == row.package.name && kind.declaration.id == row.connector
            })
            .map(|kind| &kind.declaration);
        let label = declaration
            .map(|declaration| title(&declaration.display))
            .unwrap_or_else(|| row.connector.clone());
        let account = row
            .identity
            .as_ref()
            .map(|identity| identity.name.clone())
            .unwrap_or_default();
        let status = match row.state {
            connector::State::NotConnected => Some("connector_not_connected"),
            connector::State::Expired => Some("connector_expired"),
            connector::State::NeedsConfirmation => Some("connector_confirmation"),
            connector::State::Failed => Some("connector_failed"),
            _ => None,
        };
        let row_id = row.id.to_string();
        let command = Command::SetConnectionEnabled {
            connection: row.id,
            expected_revision: row.revision,
            enabled: !row.enabled,
        };
        let icon = declaration
            .and_then(|declaration| declaration.display.icon.as_ref())
            .map(crate::assets::icons::icon)
            .unwrap_or_else(|| Icon::new(gpui_kit::assets::IconName::Plug));
        let text = v_flex()
            .min_w_0()
            .flex_1()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(label.clone())
                    .when_some(status, |element, key| {
                        element.child(Tag::new().child(tr(key)).xsmall())
                    }),
            )
            .when(!account.is_empty(), |element| {
                element.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(account),
                )
            });
        super::entry::Entry::new(
            row_id.clone(),
            h_flex()
                .gap_3()
                .items_center()
                .child(crate::plugins::emblem::surface(&label, None, px(36.)).child(icon.size_5()))
                .child(text),
        )
        .control(
            h_flex()
                .gap_2()
                .when(
                    row.state == connector::State::Connecting
                        || self.connectors.pending.contains(&row.id),
                    |element| element.child(Spinner::new().small()),
                )
                .child(
                    Switch::new(format!("connector-enabled-{row_id}"))
                        .checked(row.enabled)
                        .disabled(
                            row.state == connector::State::Connecting
                                || self.connectors.pending.contains(&row.id),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.connector_command(command.clone(), window, cx)
                        })),
                )
                .child(self.connector_menu(row, cx)),
        )
        .into_any_element()
    }

    fn refresh_connectors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.connectors.binding.clone() else {
            return;
        };
        if self.connectors.refreshing {
            return;
        }
        let mut requests = Vec::new();
        for row in &self.connectors.rows {
            if !row.enabled
                || matches!(
                    row.state,
                    connector::State::Connecting | connector::State::NeedsConfirmation
                )
                || self.connectors.pending.contains(&row.id)
            {
                continue;
            }
            if self
                .connectors
                .requests
                .get(&row.id)
                .is_some_and(|request| !matches!(&request.command, Command::ProbeConnection { .. }))
            {
                crate::feedback::error("", &tr("connector_unknown"), window, cx);
                return;
            }
            let request = self
                .connectors
                .requests
                .entry(row.id)
                .or_insert_with(|| {
                    binding
                        .client
                        .prepare(Command::ProbeConnection { connection: row.id })
                })
                .clone();
            requests.push((row.id, request));
        }
        if requests.is_empty() {
            self.connectors.close();
            self.warm_connectors(window, cx);
            return;
        }
        self.connectors.refreshing = true;
        self.connectors
            .pending
            .extend(requests.iter().map(|(id, _)| *id));
        let client = binding.client.clone();
        let expected = client.target();
        let pending = requests.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let job = binding.runtime.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for (id, request) in requests {
                let client = client.clone();
                tasks.spawn(async move { (id, client.execute(request).await) });
            }
            let mut results = Vec::new();
            while let Some(result) = tasks.join_next().await {
                results.push(result);
            }
            results
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this.connectors.binding.as_ref().is_none_or(|binding| binding.client.target() != expected) { return; }
                this.connectors.refreshing = false;
                for id in pending { this.connectors.pending.remove(&id); }
                let mut fault = None;
                match result {
                    Ok(results) => for result in results {
                        match result {
                            Ok((id, result)) => {
                                this.connectors.pending.remove(&id);
                                if !matches!(&result, Err(error) if matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable)) {
                                    this.connectors.requests.remove(&id);
                                }
                                if let Err(error) = result { fault.get_or_insert(error); }
                            },
                            Err(_) => { fault.get_or_insert_with(|| Fault::new(ErrorCode::OutcomeUnknown, "connector refresh interrupted")); },
                        }
                    },
                    Err(_) => { fault = Some(Fault::new(ErrorCode::OutcomeUnknown, "connector refresh interrupted")); },
                }
                if let Some(error) = fault { failure(error, window, cx); }
                else { crate::feedback::toast(window, tr("connector_saved"), notification::Notification::success(tr("connector_saved")), cx); }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(super) fn connector_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(binding) = self.connectors.binding.clone() else {
            return;
        };
        let id = match &command {
            Command::ProbeConnection { connection }
            | Command::UpdateConnection { connection, .. }
            | Command::SetConnectionEnabled { connection, .. }
            | Command::DisconnectConnection { connection, .. }
            | Command::RemoveConnection { connection, .. }
            | Command::ConfirmConnectorConsent { connection, .. } => *connection,
            _ => return,
        };
        if self.connectors.requests.get(&id).is_some_and(|request| {
            serde_json::to_value(&request.command).ok() != serde_json::to_value(&command).ok()
        }) {
            crate::feedback::error("", &tr("connector_unknown"), window, cx);
            return;
        }
        if !self.connectors.pending.insert(id) {
            return;
        }
        let client = binding.client.clone();
        let expected = client.target();
        let request = self
            .connectors
            .requests
            .entry(id)
            .or_insert_with(|| client.prepare(command))
            .clone();
        let job = binding
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.notify();
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this.connectors.binding.as_ref().is_none_or(|binding| binding.client.target() != expected) { return; }
                this.connectors.pending.remove(&id);
                if !matches!(&result, Ok(Err(error)) if matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable)) {
                    this.connectors.requests.remove(&id);
                }
                match result {
                Ok(Ok(_)) => crate::feedback::toast(
                    window,
                    tr("connector_saved"),
                    notification::Notification::success(tr("connector_saved")),
                    cx,
                ),
                Ok(Err(error)) => failure(error, window, cx),
                _ => crate::feedback::error("", &tr("connector_unknown"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
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
