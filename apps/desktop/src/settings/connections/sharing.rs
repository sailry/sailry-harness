//! Dialog-owned debounce and code observation; Link owns live relay configuration.
use super::*;
use sailry_link::rendezvous::ShareState;
use std::time::Duration;

impl Connections {
    pub(super) fn share(&mut self, cx: &mut Context<Self>) {
        self.start_share(Duration::ZERO, cx);
    }

    pub(super) fn start_share(&mut self, debounce: Duration, cx: &mut Context<Self>) {
        self.cancel(cx);
        let Some(selection) = self.relay_selection(cx) else {
            return;
        };
        let Some((services, relay)) = self.setup(cx) else {
            return;
        };
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        self.checking = services.relay_enabled;
        self.status = tr(if self.checking {
            "pairing_relay_checking"
        } else {
            "pairing_preparing"
        });
        let revision = self.revision;
        self.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(debounce).await;
            if stop.is_cancelled() {
                return;
            }
            let link = services.link.clone();
            let chosen = selection.clone();
            let token = stop.clone();
            let enabled = services.relay_enabled;
            let probe = services.runtime.spawn(async move {
                if enabled {
                    link.select_relays(chosen, token).await
                } else {
                    Ok(())
                }
            }).await;
            if stop.is_cancelled() {
                return;
            }
            let prepared = matches!(&probe, Ok(Ok(())));
            let accepted = this.update(cx, |view, cx| {
                if view.revision != revision {
                    return false;
                }
                view.checking = false;
                if !prepared {
                    view.stop = None;
                    view.status = tr("pairing_idle");
                    view.relay_feedback = Some(if matches!(probe, Ok(Err(ref error)) if error.code == sailry_protocol::ErrorCode::InvalidRequest) {
                        "pairing_relay_invalid"
                    } else {
                        "pairing_relay_failed"
                    });
                } else {
                    view.status = tr("pairing_preparing");
                    if enabled {
                        let custom = match &selection {
                            RelaySelection::Default => None,
                            RelaySelection::Custom(urls) => Some(urls.clone()),
                        };
                        crate::preferences::update(cx, |data| data.iroh_relays = custom);
                        if matches!(selection, RelaySelection::Custom(_)) {
                            view.relay_feedback = Some("pairing_relay_ready");
                        }
                    }
                }
                cx.notify();
                prepared
            }).unwrap_or(false);
            if !accepted || stop.is_cancelled() {
                return;
            }
            let (state, mut updates) = tokio::sync::watch::channel(ShareState::Preparing);
            let link = services.link.clone();
            services.runtime.spawn(async move {
                if let Err(error) = relay.share(&link, state.clone(), stop).await {
                    state.send_replace(ShareState::Retrying(error));
                }
            });
            loop {
                let state = updates.borrow_and_update().clone();
                let active = this.update(cx, |view, cx| {
                    if view.revision != revision {
                        return false;
                    }
                    view.code = None;
                    view.status = match state {
                        ShareState::Preparing => tr("pairing_preparing"),
                        ShareState::Ready { code, .. } => {
                            view.code = Some(code);
                            tr("pairing_refresh_hint")
                        }
                        ShareState::Retrying(_) => tr("pairing_retrying"),
                        ShareState::Paired => {
                            view.stop = None;
                            cx.emit(DismissEvent);
                            tr("pairing_success")
                        }
                        ShareState::Closed => {
                            view.stop = None;
                            tr("pairing_idle")
                        }
                    };
                    cx.notify();
                    true
                }).unwrap_or(false);
                if !active {
                    break;
                }
                if updates.changed().await.is_err() {
                    let _ = this.update(cx, |view, cx| {
                        if view.revision == revision && view.stop.take().is_some() && view.code.is_none() {
                            view.status = tr("pairing_failed");
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        }));
        cx.notify();
    }
}
