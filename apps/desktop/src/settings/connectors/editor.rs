//! The form owns drafts; the Node owns authentication and connection state.
mod account;
mod choices;
mod login;
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    form::Field,
    input::InputContentType,
    tab::{Tab, TabBar},
};
use sailry_protocol::{
    Secret,
    plugin::connector::{Auth, Input as Values, Kind, LoginState},
};
use std::collections::BTreeMap;

pub(super) fn open(
    owner: Entity<Workspace>,
    connection: Option<Connection>,
    window: &mut Window,
    cx: &mut App,
) {
    mount(owner, connection, None, None, window, cx);
}
pub(in crate::settings) fn manage(
    owner: Entity<Workspace>,
    package: sailry_protocol::plugin::Reference,
    window: &mut Window,
    cx: &mut App,
) {
    owner.update(cx, |owner, cx| owner.warm_connectors(window, cx));
    let workspace = owner.read(cx);
    let connection = workspace
        .connectors
        .rows
        .iter()
        .find(|account| account.package.name == package.name)
        .cloned();
    let kind = workspace
        .connectors
        .types
        .iter()
        .find(|kind| kind.package == package)
        .cloned();
    mount(owner, connection, kind, Some(package), window, cx);
}

fn mount(
    owner: Entity<Workspace>,
    connection: Option<Connection>,
    selected: Option<Type>,
    target: Option<sailry_protocol::plugin::Reference>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(binding) = owner.read(cx).connectors.binding.clone() else {
        return;
    };
    if owner
        .read(cx)
        .provider_link
        .as_ref()
        .is_none_or(|live| live.binding.client.target() != binding.client.target())
    {
        return;
    }
    let types = owner.read(cx).connectors.types.clone();
    let editing = connection.is_some();
    let directory = !editing && selected.is_none() && target.is_none();
    let target = target
        .or_else(|| connection.as_ref().map(|account| account.package.clone()))
        .or_else(|| selected.as_ref().map(|kind| kind.package.clone()));
    let form = cx.new(|cx: &mut Context<Editor>| {
        let mut editor = Editor {
            owner,
            binding,
            types,
            connection,
            selected: None,
            target,
            directory,
            method: 0,
            fields: BTreeMap::new(),
            query: cx.new(|cx| InputState::new(window, cx).placeholder(tr("connector_search"))),
            pending: false,
            flow: None,
            attempt: None,
            request: None,
            callback: None,
            stop: CancellationToken::new(),
            task: None,
            cli: None,
            inspecting: false,
            closed: false,
            dialog: Default::default(),
            editing,
            reconnecting: false,
        };
        cx.observe(&editor.query, |_, _, cx| cx.notify()).detach();
        cx.observe_in(&editor.owner, window, |editor, _, window, cx| {
            editor.reconcile(window, cx)
        })
        .detach();
        if let Some(connection) = &editor.connection
            && let Some(index) = editor.types.iter().position(|kind| {
                kind.package.name == connection.package.name
                    && kind.declaration.id == connection.connector
            })
        {
            editor.choose(index, window, cx);
        } else if let Some(selected) = &selected
            && let Some(index) = editor.types.iter().position(|kind| {
                kind.package == selected.package && kind.declaration.id == selected.declaration.id
            })
        {
            editor.choose(index, window, cx);
        }
        editor
    });
    let active = form.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = form.clone();
        let dialog = if form.read(cx).flow.is_some() {
            crate::ui::authorization::dialog(dialog, window, cx)
        } else {
            dialog
                .floating_surface(cx)
                .w((window.viewport_size().width - px(48.)).min(px(520.)))
                .max_h(window.viewport_size().height * 0.85)
                .p_6()
        };
        dialog
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| {
                close.update(cx, |editor, _| {
                    editor.closed = true;
                    editor.stop.cancel();
                    editor.dialog.dismissed();
                })
            })
            .child(form.clone())
    });
    active.update(cx, |editor, cx| {
        crate::ui::overlays::Owned::bind(editor, |editor| &mut editor.dialog, window, cx)
    });
}

pub(super) struct Editor {
    owner: Entity<Workspace>,
    binding: Binding,
    types: Vec<Type>,
    connection: Option<Connection>,
    selected: Option<usize>,
    target: Option<sailry_protocol::plugin::Reference>,
    directory: bool,
    method: usize,
    fields: BTreeMap<String, Entity<InputState>>,
    query: Entity<InputState>,
    pending: bool,
    flow: Option<LoginState>,
    attempt: Option<connector::Attempt>,
    request: Option<sailry_protocol::Request>,
    callback: Option<sailry_client::oauth::Callback>,
    stop: CancellationToken,
    task: Option<Task<()>>,
    cli: Option<connector::Identity>,
    inspecting: bool,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
    editing: bool,
    reconnecting: bool,
}
impl Drop for Editor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Editor {
    fn configuring(&self) -> bool {
        let Some(method) = self.method().map(Auth::kind) else {
            return false;
        };
        configuring(
            method,
            self.connection.as_ref().map(|account| account.state),
            self.kind().is_some_and(|kind| {
                kind.declaration
                    .fields
                    .iter()
                    .any(|field| editable(field, method))
            }),
            self.reconnecting,
        )
    }

    fn reconcile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .owner
            .read(cx)
            .provider_link
            .as_ref()
            .is_none_or(|live| live.binding.client.target() != self.binding.client.target())
        {
            self.closed = true;
            self.stop.cancel();
            self.dialog.close(window, cx);
            return;
        }
        if self.selected.is_none() {
            self.types = self.owner.read(cx).connectors.types.clone();
        }
        if self.selected.is_none()
            && let Some(target) = &self.target
            && let Some(index) = self.types.iter().position(|kind| {
                kind.package.name == target.name
                    && self
                        .connection
                        .as_ref()
                        .is_none_or(|account| account.connector == kind.declaration.id)
            })
        {
            self.choose(index, window, cx);
        }
        if let Some(connection) = &self.connection
            && let Some(current) = self
                .owner
                .read(cx)
                .connectors
                .rows
                .iter()
                .find(|account| account.id == connection.id)
        {
            self.connection = Some(current.clone());
        }
        if self.pending || self.closed {
            return;
        }
        let Some(request) = &self.request else { return };
        let Some(current) =
            super::commands::observed_edit(request, &self.owner.read(cx).connectors.rows)
        else {
            return;
        };
        self.connection = Some(current);
        self.request = None;
        self.callback = None;
        self.flow = None;
        cx.notify();
    }

    fn kind(&self) -> Option<&Type> {
        self.selected.and_then(|index| self.types.get(index))
    }
    fn method(&self) -> Option<&Auth> {
        self.kind()
            .and_then(|kind| kind.declaration.auth.get(self.method))
    }
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || index >= self.types.len() {
            return;
        }
        if self.connection.is_none() {
            self.connection =
                choices::existing(&self.types[index], &self.owner.read(cx).connectors.rows)
                    .cloned();
            self.editing = self.connection.is_some();
        }
        self.selected = Some(index);
        self.method = self
            .connection
            .as_ref()
            .and_then(|connection| {
                self.types[index]
                    .declaration
                    .auth
                    .iter()
                    .position(|auth| auth.kind() == connection.method)
            })
            .unwrap_or(0);
        self.fields.clear();
        self.cli = None;
        for field in self.types[index].declaration.fields.clone() {
            let value = self
                .connection
                .as_ref()
                .and_then(|connection| connection.fields.get(&field.name))
                .cloned()
                .or(field.default.clone())
                .unwrap_or_default();
            let configured = self
                .connection
                .as_ref()
                .is_some_and(|connection| connection.configured.contains(&field.name));
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(if configured {
                        tr("connector_configured")
                    } else {
                        SharedString::default()
                    })
            });
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            self.fields.insert(field.name, state);
        }
        if !self.configuring()
            && self
                .method()
                .is_some_and(|auth| auth.kind() == Kind::LocalCli)
        {
            self.inspect(window, cx);
        }
        cx.notify();
    }
    fn back(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        self.target = None;
        self.connection = None;
        self.editing = false;
        self.reconnecting = false;
        self.fields.clear();
        self.cli = None;
        cx.notify();
    }

    fn required(&self, cx: &App) -> bool {
        if self.closed
            || !self.owner.read(cx).connectors.received
            || self
                .connection
                .as_ref()
                .is_some_and(|account| self.owner.read(cx).connectors.pending.contains(&account.id))
        {
            return false;
        }
        let Some(kind) = self.kind() else {
            return false;
        };
        if !kind.enabled && !self.configuring() {
            return false;
        }
        if self
            .connection
            .as_ref()
            .is_some_and(|account| account.state == connector::State::NeedsConfirmation)
        {
            return true;
        }
        if !self.configuring() {
            if self
                .method()
                .is_some_and(|auth| auth.kind() == Kind::LocalCli)
            {
                return self.cli.is_some() && !self.inspecting;
            }
            if self
                .method()
                .is_some_and(|auth| matches!(auth, Auth::McpOauth { client: None, .. }))
            {
                return true;
            }
        }
        kind.declaration
            .fields
            .iter()
            .filter(|field| editable(field, self.method().expect("selected method").kind()))
            .filter(|field| !field.optional)
            .all(|field| {
                self.fields
                    .get(&field.name)
                    .is_some_and(|state| !state.read(cx).value().trim().is_empty())
                    || (field.secret
                        && self
                            .connection
                            .as_ref()
                            .is_some_and(|connection| connection.configured.contains(&field.name)))
            })
    }
    fn values(&self, cx: &App) -> Values {
        let mut input = Values::default();
        if let Some(kind) = self.kind() {
            for field in &kind.declaration.fields {
                if !editable(field, self.method().expect("selected method").kind()) {
                    continue;
                }
                let value = self
                    .fields
                    .get(&field.name)
                    .map(|state| state.read(cx).value().to_string())
                    .unwrap_or_default();
                if field.secret {
                    if !value.is_empty() {
                        input.secrets.insert(field.name.clone(), Secret::new(value));
                    }
                } else {
                    input.fields.insert(field.name.clone(), value);
                }
            }
        }
        input
    }
    fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.kind().cloned() else {
            return;
        };
        self.inspecting = true;
        self.cli = None;
        let expected = (kind.package.clone(), kind.declaration.id.clone());
        let client = self.binding.client.clone();
        let job = self.binding.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::InspectConnectorCli {
                    package: kind.package,
                    connector: kind.declaration.id,
                }))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this.closed
                    || this.kind().is_none_or(|kind| {
                        kind.package != expected.0 || kind.declaration.id != expected.1
                    })
                    || this
                        .method()
                        .is_none_or(|auth| auth.kind() != Kind::LocalCli)
                {
                    return;
                }
                this.inspecting = false;
                match result {
                    Ok(Ok(Output::ConnectorIdentity(identity))) => {
                        this.cli = Some(identity);
                    }
                    Ok(Err(error)) => failure(error, window, cx),
                    _ => crate::feedback::error("", &tr("connector_unknown"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn field_form(&self, kind: &Type) -> AnyElement {
        let mut form = crate::ui::form::vertical();
        for field in &kind.declaration.fields {
            if !editable(field, self.method().expect("selected method").kind()) {
                continue;
            }
            let input = Input::new(self.fields.get(&field.name).expect("connector field state"))
                .when(field.secret, |input| {
                    input.content_type(InputContentType::Password).mask_toggle()
                });
            let label = field
                .locales
                .get::<str>(rust_i18n::locale().as_ref())
                .cloned()
                .unwrap_or_else(|| field.title.clone());
            let help = field.help.clone();
            let name = field.name.clone();
            let element = Field::new()
                .label_fn(move |_, _| {
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(label.clone())
                        .when_some(help.clone(), |element, url| {
                            element.child(
                                Button::new(format!("connector-help-{name}"))
                                    .link()
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .accessibility_label(tr("menu_help"))
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                        })
                })
                .child(input);
            form = form.child(element);
        }
        form.into_any_element()
    }

    fn normal(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(kind) = self.kind() else {
            return if self.target.is_some() {
                Spinner::new().into_any_element()
            } else {
                self.choices(cx)
            };
        };
        let auth = self.method().expect("selected connector method");
        let title = title(&kind.declaration.display);
        let selected = self.method;
        let header = h_flex()
            .gap_3()
            .items_center()
            .when(self.directory, |header| {
                header.child(
                    Button::new("connector-back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .disabled(self.pending || self.request.is_some())
                        .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                )
            })
            .child(
                crate::plugins::emblem::surface(&title, None, px(32.))
                    .child(Icon::new(gpui_kit::assets::IconName::Plug).size_4()),
            )
            .child(div().text_lg().font_semibold().child(title));
        let methods = TabBar::new("connector-methods")
            .segmented()
            .selected_index(selected)
            .children(kind.declaration.auth.iter().map(|auth| {
                Tab::new()
                    .label(tr(filter::key(auth.kind())))
                    .disabled(self.editing || self.pending || self.request.is_some())
            }))
            .on_click(cx.listener(|this, index, window, cx| {
                this.method = *index;
                this.cli = None;
                if this
                    .method()
                    .is_some_and(|auth| auth.kind() == Kind::LocalCli)
                {
                    this.inspect(window, cx);
                }
                cx.notify();
            }));
        let content = if self.configuring() {
            self.field_form(kind)
        } else {
            match auth {
                Auth::LocalCli { .. } => v_flex()
                    .gap_3()
                    .when(!self.inspecting, |element| {
                        element
                            .when(self.cli.is_some(), |element| {
                                element.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr("connector_choose_account")),
                                )
                            })
                            .child(account::list(self.cli.as_ref(), cx))
                    })
                    .into_any_element(),
                Auth::McpOauth { client: None, .. }
                | Auth::Oauth2 {
                    client: connector::Client::Bundled { .. },
                    ..
                } => v_flex()
                    .items_center()
                    .gap_3()
                    .child(Icon::new(IconName::Globe).size_8())
                    .child(tr("connector_browser"))
                    .into_any_element(),
                _ => self.field_form(kind),
            }
        };
        let reacquire = self.configuring()
            && matches!(auth.kind(), Kind::Oauth2 | Kind::McpOauth | Kind::LocalCli);
        let confirming = self
            .connection
            .as_ref()
            .is_some_and(|account| account.state == connector::State::NeedsConfirmation);
        // The local CLI account list already names the bound identity.
        let local = !self.configuring() && auth.kind() == Kind::LocalCli;
        let registration = matches!(
            auth,
            Auth::Application {
                registration: Some(_),
                ..
            }
        );
        v_flex()
            .w_full()
            .gap_5()
            .child(header)
            .when(
                kind.declaration.auth.len() > 1 && !self.editing,
                |element| element.child(methods),
            )
            .when_some(
                self.connection
                    .as_ref()
                    .and_then(|account| account.identity.as_ref())
                    .filter(|_| !local),
                |body, identity| {
                    body.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(identity.name.clone()),
                    )
                },
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(content)
                    .when(reacquire, |element| {
                        element.child(
                            Button::new("connector-reacquire")
                                .ghost()
                                .small()
                                .label(tr(if auth.kind() == Kind::LocalCli {
                                    "connector_bind"
                                } else {
                                    "connector_reauthorize"
                                }))
                                .disabled(self.pending || !kind.enabled)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reconnecting = true;
                                    if this
                                        .method()
                                        .is_some_and(|method| method.kind() == Kind::LocalCli)
                                    {
                                        this.inspect(window, cx);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .when(registration, |element| {
                        element.child(
                            Button::new("connector-register")
                                .outline()
                                .small()
                                .label(tr("connector_register"))
                                .disabled(self.pending)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.submit(true, window, cx)
                                    }),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("connector-cancel")
                            .ghost()
                            .label(tr("authorization_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("connector-submit")
                            .primary()
                            .label(tr(if confirming {
                                "connector_confirm"
                            } else {
                                submit_key(auth.kind(), self.configuring(), self.editing)
                            }))
                            .loading(self.pending)
                            .disabled(self.pending || !self.required(cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit(false, window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn configuring(
    method: Kind,
    state: Option<connector::State>,
    fields: bool,
    reacquire: bool,
) -> bool {
    state.is_some()
        && fields
        && !reacquire
        && (!matches!(method, Kind::Oauth2 | Kind::McpOauth)
            || state == Some(connector::State::Connected))
}

fn submit_key(method: Kind, configuring: bool, existing: bool) -> &'static str {
    if configuring {
        return "settings_save";
    }
    match method {
        Kind::Oauth2 | Kind::McpOauth if existing => "connector_reauthorize",
        Kind::Oauth2 | Kind::McpOauth => "connector_authorize",
        Kind::LocalCli => "connector_bind",
        Kind::Secret | Kind::Application => "settings_save",
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closed {
            div().into_any_element()
        } else if self.flow.is_some() {
            self.authorization(cx)
        } else {
            self.normal(cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{configuring, submit_key};
    use sailry_protocol::plugin::connector::{Kind, State};

    #[test]
    fn existing_oauth_requires_an_explicit_action() {
        for method in [Kind::Oauth2, Kind::McpOauth] {
            assert!(!configuring(method, Some(State::Connected), false, false));
            assert!(configuring(method, Some(State::Connected), true, false));
            assert!(!configuring(method, Some(State::Connected), true, true));
            assert_eq!(submit_key(method, false, true), "connector_reauthorize");
            assert_eq!(submit_key(method, false, false), "connector_authorize");
            for state in [
                State::Expired,
                State::NotConnected,
                State::NeedsConfirmation,
            ] {
                assert!(!configuring(method, Some(state), true, false));
            }
        }
    }

    #[test]
    fn preserves_token_and_cli_actions() {
        for method in [Kind::Secret, Kind::Application] {
            assert!(configuring(method, Some(State::Connected), true, false));
            assert_eq!(submit_key(method, true, true), "settings_save");
            assert_eq!(submit_key(method, false, false), "settings_save");
        }
        assert!(!configuring(
            Kind::LocalCli,
            Some(State::Connected),
            false,
            false
        ));
        assert_eq!(submit_key(Kind::LocalCli, false, true), "connector_bind");
        assert!(!configuring(Kind::Secret, None, true, false));
    }
}
