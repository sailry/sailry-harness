//! The form owns drafts; the Node owns authentication and connection state.
mod choices;
mod login;
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    form::{Field, Form},
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
    mount(owner, connection, false, window, cx);
}
pub(super) fn reconnect(
    owner: Entity<Workspace>,
    connection: Connection,
    window: &mut Window,
    cx: &mut App,
) {
    mount(owner, Some(connection), true, window, cx);
}
fn mount(
    owner: Entity<Workspace>,
    connection: Option<Connection>,
    reconnect: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(binding) = owner.read(cx).connectors.binding.clone() else {
        return;
    };
    let types = owner.read(cx).connectors.types.clone();
    let editing = connection.is_some();
    let form = cx.new(|cx: &mut Context<Editor>| {
        let mut editor = Editor {
            owner,
            binding,
            types,
            connection,
            selected: None,
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
            editing,
            reconnecting: reconnect,
        };
        cx.observe(&editor.query, |_, _, cx| cx.notify()).detach();
        cx.observe(&editor.owner, |editor, _, cx| editor.reconcile(cx))
            .detach();
        if let Some(connection) = &editor.connection
            && let Some(index) = editor.types.iter().position(|kind| {
                kind.package.name == connection.package.name
                    && kind.declaration.id == connection.connector
            })
        {
            editor.choose(index, window, cx);
        }
        if reconnect
            && editor
                .method()
                .is_some_and(|auth| matches!(auth, Auth::Oauth2 { .. } | Auth::McpOauth { .. }))
        {
            editor.submit(false, window, cx);
        }
        editor
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = form.clone();
        let dialog = if form.read(cx).flow.is_some() {
            crate::ui::authorization::dialog(dialog, window, cx)
        } else {
            dialog
                .floating_surface(cx)
                .w((window.viewport_size().width - px(48.)).min(px(520.)))
                .p_6()
        };
        dialog
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| {
                close.update(cx, |editor, _| {
                    editor.closed = true;
                    editor.stop.cancel();
                })
            })
            .child(form.clone())
    });
}

pub(super) struct Editor {
    owner: Entity<Workspace>,
    binding: Binding,
    types: Vec<Type>,
    connection: Option<Connection>,
    selected: Option<usize>,
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
        self.editing && !self.reconnecting
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
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
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            window.close_dialog(cx);
        } else {
            self.selected = None;
            self.fields.clear();
            self.cli = None;
            cx.notify();
        }
    }
    fn required(&self, cx: &App) -> bool {
        let Some(kind) = self.kind() else {
            return false;
        };
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
        let mut form = Form::new();
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
            return self.choices(cx);
        };
        let auth = self.method().expect("selected connector method");
        let title = title(&kind.declaration.display);
        let selected = self.method;
        let header = h_flex()
            .gap_3()
            .items_center()
            .when(!self.editing, |header| {
                header.child(
                    Button::new("connector-back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
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
                    .disabled(self.editing)
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
                    .when(self.inspecting, |element| {
                        element.child(Skeleton::new().h_12().w_full())
                    })
                    .when(!self.inspecting, |element| {
                        element.child(
                            h_flex()
                                .gap_3()
                                .child(Icon::new(if self.cli.is_some() {
                                    IconName::CircleCheck
                                } else {
                                    IconName::CircleAlert
                                }))
                                .child(
                                    self.cli
                                        .as_ref()
                                        .map(|identity| identity.name.clone())
                                        .unwrap_or_else(|| {
                                            tr("connector_not_connected").to_string()
                                        }),
                                ),
                        )
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
            .child(
                v_flex()
                    .w_full()
                    .min_h(px(300.))
                    .gap_3()
                    .child(content)
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
                            .disabled(self.pending)
                            .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                    )
                    .child(
                        Button::new("connector-submit")
                            .primary()
                            .label(tr(submit_key(auth.kind(), self.configuring())))
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

fn submit_key(method: Kind, configuring: bool) -> &'static str {
    if configuring {
        return "settings_save";
    }
    match method {
        Kind::Oauth2 | Kind::McpOauth => "connector_authorize",
        Kind::LocalCli => "connector_bind",
        Kind::Secret | Kind::Application => "settings_save",
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.flow.is_some() {
            self.authorization(cx)
        } else {
            self.normal(cx)
        }
    }
}
