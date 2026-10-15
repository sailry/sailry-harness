use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, OtpInput, OtpState},
    select::{Select, SelectEvent, SelectState},
    *,
};
use gpui_kit::*;
use sailry_link::{
    CancellationToken, RelaySelection,
    rendezvous::{Relay, RequestId},
};

use super::group::Group;
mod devices;
mod dialog;
mod relay;
mod sharing;
use crate::{backend::Services, tr};
use gpui_kit::prelude::FluentBuilder as _;

pub struct Connections {
    endpoint: String,
    pin: Entity<OtpState>,
    display: Entity<OtpState>,
    dialog: Option<FocusHandle>,
    devices: Vec<devices::Device>,
    watch_stop: Option<CancellationToken>,
    watcher: Option<Task<()>>,
    status: SharedString,
    code: Option<String>,
    stop: Option<CancellationToken>,
    task: Option<Task<()>>,
    claim: Option<(String, RequestId)>,
    relay_select: Entity<SelectState<Vec<relay::Network>>>,
    relay_address: Entity<InputState>,
    relay_feedback: Option<&'static str>,
    sharing: bool,
    checking: bool,
    revision: u64,
}

impl Connections {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |this: &Self, _| {
                [
                    "pairing_failed",
                    "pairing_retrying",
                    "pairing_invalid_endpoint",
                    "pairing_success",
                ]
                .into_iter()
                .filter(|key| this.status == tr(key))
                .chain(this.relay_feedback)
                .collect()
            },
            |_, key, _| {
                if matches!(key, "pairing_success" | "pairing_relay_ready") {
                    gpui_kit::component::notification::Notification::info(tr(key))
                } else {
                    gpui_kit::component::notification::Notification::error(tr(key))
                }
            },
        );
        cx.observe_global::<crate::preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        cx.subscribe_in(
            &cx.entity(),
            window,
            |this, _, _: &DismissEvent, window, cx| {
                if this
                    .dialog
                    .as_ref()
                    .is_some_and(|focus| focus.contains_focused(window, cx))
                {
                    this.dialog = None;
                    window.close_dialog(cx);
                }
            },
        )
        .detach();
        let pin = cx.new(|cx| OtpState::new(6, window, cx));
        cx.observe(&pin, |_, _, cx| cx.notify()).detach();
        let (relay_select, relay_address) = relay::fields(window, cx);
        Self {
            endpoint: crate::preferences::data(cx).pairing,
            pin,
            display: cx.new(|cx| OtpState::new(6, window, cx)),
            dialog: None,
            devices: vec![],
            watch_stop: None,
            watcher: None,
            status: tr("pairing_idle"),
            code: None,
            stop: None,
            task: None,
            claim: None,
            relay_select,
            relay_address,
            relay_feedback: None,
            sharing: false,
            checking: false,
            revision: 0,
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
        self.task = None;
        self.code = None;
        self.status = tr("pairing_idle");
        self.checking = false;
        self.relay_feedback = None;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.cancel(cx);
        self.sharing = false;
        if let Some(stop) = self.watch_stop.take() {
            stop.cancel();
        }
        self.watcher = None;
    }

    fn setup(&mut self, cx: &mut Context<Self>) -> Option<(Services, Relay)> {
        let Some(services) = cx.try_global::<Services>().cloned() else {
            self.status = tr("pairing_preview_disabled");
            cx.notify();
            return None;
        };
        match Relay::new(if self.endpoint.trim().is_empty() {
            sailry_link::rendezvous::DEFAULT_SERVICE
        } else {
            &self.endpoint
        }) {
            Ok(relay) => Some((services, relay)),
            Err(_) => {
                self.status = tr("pairing_invalid_endpoint");
                cx.notify();
                None
            }
        }
    }

    fn pair(&mut self, cx: &mut Context<Self>) {
        self.cancel(cx);
        let Some((services, relay)) = self.setup(cx) else {
            return;
        };
        let pin = self.pin.read(cx).value().to_string();
        if self
            .claim
            .as_ref()
            .is_none_or(|(previous, _)| previous != &pin)
        {
            self.claim = Some((pin.clone(), RequestId::default()));
        }
        let request = self.claim.as_ref().unwrap().1.clone();
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        self.status = tr("pairing_preparing");
        let job = services.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = relay.pair(&services.link, &pin, &request) => Some(result),
            }
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.stop = None;
                this.status = if matches!(result, Ok(Some(Ok(_)))) {
                    cx.emit(DismissEvent);
                    tr("pairing_success")
                } else {
                    tr("pairing_failed")
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        if let Some(stop) = self.watch_stop.take() {
            stop.cancel();
        }
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
    }
}

impl Render for Connections {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.observe_devices(cx);
        let live = cx.try_global::<Services>().is_some();
        v_flex()
            .gap_6()
            .child(
                Group::new("connections_hosts")
                    .action(
                        Button::new("pairing-share")
                            .debug_selector(|| "pairing-share".into())
                            .label(tr("pairing_share"))
                            .disabled(!live)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(true, window, cx)
                            })),
                    )
                    .action(
                        Button::new("pairing-add")
                            .debug_selector(|| "pairing-add".into())
                            .primary()
                            .label(tr("settings_add"))
                            .disabled(!live)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(false, window, cx)
                            })),
                    )
                    .children(self.device_rows(true, cx)),
            )
            .child(Group::new("connections_mobile").children(self.device_rows(false, cx)))
    }
}
