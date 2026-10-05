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
use gpui_kit::prelude::FluentBuilder as _;
#[cfg(test)]
#[path = "connections/live_test.rs"]
mod live_test;
use crate::{backend::Services, tr};

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

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;

    struct Frame(Entity<Connections>);
    impl Render for Frame {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(self.0.clone())
        }
    }

    #[gpui::test]
    fn retains_pairing_draft_after_feedback(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Connections::new(window, cx));
            owner = Some(view.clone());
            Root::new(cx.new(|_| Frame(view)), window, cx)
        });
        let view = owner.unwrap();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_dialog(false, window, cx);
                view.pin
                    .update(cx, |pin, cx| pin.set_value("123456", window, cx));
            });
        });
        for key in ["pairing_failed", "pairing_success"] {
            visual.update(|window, cx| {
                window.clear_notifications(cx);
                view.update(cx, |view, cx| {
                    view.status = tr(key);
                    cx.notify();
                });
            });
            crate::feedback::tests::shown(visual);
            assert_eq!(visual.update(crate::feedback::tests::summary), tr(key));
            assert_eq!(
                view.read_with(visual, |view, cx| view.pin.read(cx).value().clone()),
                "123456"
            );
            assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
            let notifications = visual.update(|window, cx| window.notifications(cx));
            view.update(visual, |_, cx| cx.notify());
            visual.run_until_parked();
            assert_eq!(
                visual.update(|window, cx| window.notifications(cx)),
                notifications
            );
        }
    }

    #[gpui::test]
    fn preview_isolation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Connections::new(window, cx));
            view.update(cx, |view, cx| {
                view.share(cx);
                assert!(view.task.is_none());
                assert!(view.stop.is_none());
                assert_eq!(view.status, tr("pairing_preview_disabled"));
                view.pair(cx);
                assert!(view.task.is_none());
            });
            Root::new(view, window, cx)
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn relay_choice_clears_old_codes_and_preserves_drafts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Connections::new(window, cx));
            owner = Some(view.clone());
            Root::new(cx.new(|_| Frame(view)), window, cx)
        });
        let view = owner.unwrap();
        let token = CancellationToken::new();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.endpoint = "https://pairing.example.test".into();
                view.open_dialog(true, window, cx);
                assert_eq!(view.relay_selection(cx), Some(RelaySelection::Default));
                view.code = Some("123456".into());
                view.stop = Some(token.clone());
            });
            let _ = window.draw(cx);
        });
        assert!(visual.debug_bounds("pairing-relay-address").is_none());
        visual.update(|window, cx| {
            let select = window.find(("select", view.read(cx).relay_select.entity_id()));
            assert_eq!(select.label(), Some(tr("pairing_relay_server").as_ref()));
            assert_eq!(select.value(), Some(tr("pairing_relay_iroh").as_ref()));
        });
        let selector = visual.debug_bounds("pairing-relay-select").unwrap();
        visual.simulate_click(selector.center(), Modifiers::default());
        visual.simulate_keystrokes("down enter");
        visual.run_until_parked();
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(visual.debug_bounds("pairing-relay-address").is_some());
        visual.update(|window, cx| {
            let select = window.find(("select", view.read(cx).relay_select.entity_id()));
            assert_eq!(select.label(), Some(tr("pairing_relay_server").as_ref()));
            assert_eq!(select.value(), Some(tr("pairing_relay_custom").as_ref()));
        });
        assert!(token.is_cancelled());
        assert!(view.read_with(visual, |view, cx| view.custom_relay(cx)
            && view.code.is_none()
            && view.stop.is_none()
            && view.task.is_none()));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.relay_address.update(cx, |input, cx| {
                    input.set_value("https://relay.example.test", window, cx);
                });
            });
        });
        visual.run_until_parked();
        assert!(view.read_with(visual, |view, cx| view.relay_selection(cx)
            == Some(RelaySelection::Custom(vec![
                "https://relay.example.test".into()
            ]))));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.relay_select.update(cx, |select, cx| {
                    select.set_selected_value(&relay::Network::Iroh, window, cx);
                    cx.emit(SelectEvent::Confirm(Some(relay::Network::Iroh)));
                });
            });
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(visual.debug_bounds("pairing-relay-address").is_none());
        assert!(view.read_with(visual, |view, cx| view.relay_selection(cx)
            == Some(RelaySelection::Default)
            && view.relay_address.read(cx).value() == "https://relay.example.test"
            && view.endpoint == "https://pairing.example.test"
            && view.task.is_none()));
    }

    #[gpui::test]
    fn clears_closed_pairing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Connections::new(window, cx));
            view.update(cx, |view, cx| {
                let stop = CancellationToken::new();
                view.stop = Some(stop.clone());
                view.code = Some("123456".into());
                view.close(cx);
                assert!(stop.is_cancelled());
                assert!(view.code.is_none());
                assert!(view.stop.is_none());
            });
            Root::new(view, window, cx)
        });
    }
}
