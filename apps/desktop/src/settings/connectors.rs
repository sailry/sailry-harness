//! Connection presentation uses the same Client contracts for local and remote Nodes.
mod commands;
mod consent;
mod editor;
mod filter;
mod menu;
use super::{Workspace, providers::Binding};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        select::{Select, SelectEvent, SelectState},
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
    method: Entity<SelectState<Vec<filter::Method>>>,
    focus: FocusHandle,
    types: Vec<Type>,
    rows: Vec<Connection>,
    binding: Option<Binding>,
    loading: bool,
    refreshing: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
    pending: BTreeSet<connector::Id>,
    requests: BTreeMap<connector::Id, commands::Submission>,
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

    pub(super) fn heading(&self) -> SharedString {
        rust_i18n::t!("connector_title", count = self.rows.len()).into()
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
                                commands::reconcile(
                                    &mut this.connectors.requests,
                                    &this.connectors.pending,
                                    &this.connectors.rows,
                                );
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
        let method = self
            .connectors
            .method
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or_default();
        let rows = self
            .connectors
            .rows
            .iter()
            .filter(|row| method.matches(row.method))
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
            .track_focus(&self.connectors.focus)
            .on_action(cx.listener(Self::connector_action))
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
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(div().truncate().child(label.clone()))
                    .child(
                        Tag::secondary()
                            .child(tr(filter::key(row.method)))
                            .xsmall()
                            .flex_shrink_0(),
                    )
                    .when_some(status, |element, key| {
                        element.child(Tag::new().child(tr(key)).xsmall().flex_shrink_0())
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
        let content = super::entry::Entry::new(
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
        );
        let row = row.clone();
        div()
            .id(SharedString::from(format!("connector-context-{row_id}")))
            .child(content)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.connectors.focus.focus(window, cx);
                    this.connector_context(&row, event.position, window, cx);
                }),
            )
            .into_any_element()
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
