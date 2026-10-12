//! Native capability controls preserve user gestures and keep OAuth payloads private.
use super::super::host::sdk::values::{decode, encode};
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::Request;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Action {
    Connect {
        target: Option<String>,
        mode: Option<card::Mode>,
    },
    Cancel,
    Refresh,
    Browser {
        target: String,
        mode: card::Mode,
    },
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LoginState {
    #[default]
    Idle,
    Starting,
    Pending,
    Exchanging,
    Connected,
    Cancelled,
    Failed,
    Unconfirmed,
}

#[derive(Clone, Serialize)]
struct State {
    cursor: String,
    props: Value,
    configured: bool,
    login: LoginState,
    can_open: bool,
    can_cancel: bool,
    action: Option<String>,
}

pub(super) struct Host {
    pub summary: card::Summary,
    pub declaration: card::Declaration,
    pub binding: Binding,
    context: Mutex<plugin::Context>,
    state: tokio::sync::watch::Sender<State>,
    active: AtomicBool,
    stop: CancellationToken,
    qr: Mutex<Option<(String, qr::Bitmap)>>,
}

impl Host {
    pub(super) fn new(
        binding: Binding,
        summary: &card::Summary,
        declaration: card::Declaration,
        reference: plugin::Reference,
    ) -> Arc<Self> {
        Arc::new(Self {
            context: Mutex::new(plugin::Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Card(summary.receipt),
                package: reference,
                worktree: Some(summary.worktree),
                session: Some(summary.receipt.session),
            }),
            state: tokio::sync::watch::channel(State {
                cursor: "0".into(),
                props: summary.props.clone(),
                configured: summary.props["configured"].as_bool().unwrap_or(false),
                login: Default::default(),
                can_open: false,
                can_cancel: false,
                action: None,
            })
            .0,
            summary: summary.clone(),
            declaration,
            binding,
            active: AtomicBool::new(true),
            stop: CancellationToken::new(),
            qr: Mutex::new(None),
        })
    }

    pub(super) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire) && !self.stop.is_cancelled()
    }
    fn check(&self) -> Result<(), HostError> {
        self.active()
            .then_some(())
            .ok_or_else(|| HostError::new("card is no longer available"))
    }
    pub(super) fn close(&self) {
        self.active.store(false, Ordering::Release);
        self.stop.cancel();
    }
    pub(super) fn reference(&self, reference: plugin::Reference) {
        if let Ok(mut context) = self.context.lock() {
            if reference.settings_revision >= context.package.settings_revision {
                context.package = reference;
            }
        }
    }
    pub(super) fn request(&self, command: Command) -> Result<Request, HostError> {
        self.check()?;
        let context = self
            .context
            .lock()
            .map_err(|_| HostError::new("card scope is unavailable"))?
            .clone();
        Ok(self.binding.client.prepare(command).with_plugin(context))
    }
    pub(super) fn package(&self) -> Result<plugin::Reference, HostError> {
        Ok(self
            .context
            .lock()
            .map_err(|_| HostError::new("card scope is unavailable"))?
            .package
            .clone())
    }
    fn change(&self, update: impl FnOnce(&mut State)) {
        self.state.send_modify(|state| {
            let next = state.cursor.parse::<u64>().unwrap_or(0).wrapping_add(1);
            update(state);
            state.cursor = next.to_string();
        });
    }
    pub(super) fn login(&self, state: LoginState, can_open: bool, can_cancel: bool) {
        let connected = matches!(state, LoginState::Connected);
        self.change(|value| {
            value.login = state;
            value.can_open = can_open;
            value.can_cancel = can_cancel;
            if connected {
                value.configured = true;
            }
        });
    }
    pub(super) fn refresh(&self, action: String) {
        self.change(|state| state.action = Some(format!("{action}:{}", state.cursor)));
    }

    pub(super) fn target(&self, name: &str, mode: card::Mode) -> Result<card::Target, HostError> {
        self.declaration
            .capabilities
            .browser
            .iter()
            .find(|target| target.name == name && target.modes.contains(&mode))
            .cloned()
            .ok_or_else(|| HostError::new("browser target is not declared"))
    }

    fn permits(&self, action: &Action) -> bool {
        match action {
            Action::Connect { target, mode } => {
                self.declaration.mcp_server.is_some()
                    && match (target, mode) {
                        (None, None) => true,
                        (Some(target), Some(mode)) => {
                            self.target(target, *mode).is_ok_and(|target| {
                                matches!(target.source, card::Source::McpAuthorization)
                            })
                        }
                        _ => false,
                    }
            }
            Action::Cancel => self.declaration.mcp_server.is_some(),
            Action::Refresh => self.declaration.capabilities.refresh,
            Action::Browser { target, mode } => self.target(target, *mode).is_ok(),
        }
    }

    pub(super) fn props(&self) -> Value {
        self.state.borrow().props.clone()
    }

    pub(super) fn module(self: &Arc<Self>, owner: WeakEntity<Card>) -> HostModule {
        let state = self.clone();
        let theme = self.clone();
        let status = self.clone();
        let changed = self.clone();
        let props = self.clone();
        let buttons = self.clone();
        let qr = self.clone();
        let toast = self.clone();
        let toast_owner = owner.clone();
        HostModule::new("sailry/card")
            .function("state", move |_| {
                state.check()?;
                let mut value = serde_json::to_value(state.state.borrow().clone()).map_err(|error| HostError::new(error.to_string()))?;
                value["locale"] = json!(rust_i18n::locale().to_string());
                encode(value)
            })
            .function("theme", move |_| {
                theme.check()?;
                gpui_shell::with_current_app(|cx| super::super::theme::snapshot(cx)).ok_or_else(|| HostError::new("card theme is unavailable"))
            })
            .async_function("status", move |_| {
                status.check()?;
                let server = status.declaration.mcp_server.clone().ok_or_else(|| HostError::new("card has no MCP binding"))?;
                let request = status.request(Command::ReadMcpAuthorization { package: status.package()?, server })?;
                let host = status.clone();
                Ok(async move {
                    let client = host.binding.client.clone();
                    let job = host.binding.runtime.spawn(async move { client.execute(request).await });
                    let output = tokio::select! {
                        _ = host.stop.cancelled() => return Err(HostError::new("card is closed")),
                        output = job => output.map_err(|_| HostError::new("card status worker failed"))?,
                    }.map_err(|_| HostError::new("MCP status could not be read"))?;
                    let Output::McpAuthorization(status) = output else { return Err(HostError::new("MCP status response expected")) };
                    host.change(|state| state.configured = status.configured);
                    encode(json!({"configured":status.configured}))
                })
            })
            .async_function("nextChange", move |args| {
                changed.check()?;
                let seen = args.string(0)?.to_owned();
                let mut receiver = changed.state.subscribe();
                let stop = changed.stop.clone();
                Ok(async move {
                    loop {
                        let state = receiver.borrow_and_update().clone();
                        if state.cursor != seen { return encode(serde_json::to_value(state).map_err(|error| HostError::new(error.to_string()))?); }
                        tokio::select! {
                            _ = stop.cancelled() => return Err(HostError::new("card is closed")),
                            result = receiver.changed() => result.map_err(|_| HostError::new("card is closed"))?,
                        }
                    }
                })
            })
            .function("publishProps", move |args| {
                props.check()?;
                let value = decode(args.value(0)?)?;
                if !props.declaration.accepts(&value) { return Err(HostError::new("card properties do not match their schema")) }
                props.change(|state| state.props = value);
                Ok(HostValue::Null)
            })
            .function("toast", move |args| {
                toast.check()?;
                let message = args.string(0)?.to_owned();
                if message.is_empty() || message.len() > 1024 { return Err(HostError::new("invalid card toast")) }
                let error = args.get(1).and_then(HostValue::as_bool).unwrap_or(false);
                let owner = toast_owner.clone();
                gpui_shell::with_current_app(|cx| cx.defer(move |cx| {
                    let _ = owner.update(cx, |card, cx| { card.pending_toast = Some((message, error)); cx.notify(); });
                })).ok_or_else(|| HostError::new("card toast is unavailable"))?;
                Ok(HostValue::Null)
            })
            .component("Action", move |args, _, _| {
                let action = args.props().get("action").and_then(|value| decode(value).ok())
                    .and_then(|value| serde_json::from_value::<Action>(value).ok());
                let Some(action) = action.filter(|action| buttons.permits(action)) else { return div().into_any_element() };
                let label = args.props().get("label").and_then(HostValue::as_str).unwrap_or_default().to_owned();
                let disabled = args.props().get("disabled").and_then(HostValue::as_bool).unwrap_or(false) || !buttons.active();
                let primary = args.props().get("primary").and_then(HostValue::as_bool).unwrap_or(false);
                let id = args.id().to_owned();
                let selector = id.clone();
                let host = buttons.clone();
                let owner = owner.clone();
                Button::new(SharedString::from(id.clone())).debug_selector(move || selector.clone())
                    .map(|button| if primary { button.primary() } else { button.ghost() })
                    .small().label(label.clone()).accessibility_label(label).disabled(disabled)
                    .on_click(move |_, window, cx| {
                        let host = host.clone(); let action = action.clone(); let id = id.clone();
                        let _ = owner.update(cx, |card, cx| card.action(host, action, id, window, cx));
                    }).into_any_element()
            })
            .component("Qr", move |_, _, _| {
                if !qr.active() { return div().into_any_element() }
                let Some(declaration) = &qr.declaration.capabilities.qr else { return div().into_any_element() };
                let props = qr.props();
                let expiry = declaration.expires_at.as_ref().and_then(|pointer| props.pointer(pointer)).and_then(Value::as_i64);
                if !super::qr::live(expiry, chrono::Utc::now().timestamp_millis()) { return div().into_any_element() }
                let Some(payload) = props.pointer(&declaration.payload).and_then(Value::as_str) else { return div().into_any_element() };
                let Ok(mut cache) = qr.qr.lock() else { return div().into_any_element() };
                if cache.as_ref().is_none_or(|(previous, _)| previous != payload) {
                    let Ok(bitmap) = super::qr::render(payload) else { return div().into_any_element() };
                    *cache = Some((payload.to_owned(), bitmap));
                }
                let bitmap = &cache.as_ref().expect("QR cache was populated").1;
                img(bitmap.image.clone()).w(px(bitmap.width as f32)).h(px(bitmap.width as f32))
                    .max_w_full().object_fit(ObjectFit::Contain).into_any_element()
            })
            .declarations(include_str!("sdk.d.ts"))
    }
}
