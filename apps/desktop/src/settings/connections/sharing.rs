//! Dialog-owned debounce and code observation; Link owns live relay configuration.
use super::*;
use sailry_client::Client;
use sailry_link::rendezvous::ShareState;
use sailry_protocol::{Command, ErrorCode, Fault, Output, network::Settings};
use std::time::Duration;

async fn prepare(
    client: &Client,
    selection: RelaySelection,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let Output::NetworkSettings(current) = client
        .execute(client.prepare(Command::ReadNetworkSettings))
        .await?
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    let selection = edit::preserve_relays(edit::desired(selection), &current);
    if current.selection.as_ref() == Some(&selection)
        && current.effective.as_ref() == Some(&selection)
    {
        return Ok(());
    }
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "pairing preparation cancelled",
        ));
    }
    let Output::NetworkSettings(Settings { effective, .. }) = client
        .execute(client.prepare(Command::SaveNetworkSettings {
            expected_revision: current.revision,
            selection: selection.clone(),
        }))
        .await?
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if effective.as_ref() != Some(&selection) {
        return Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "network selection could not be confirmed",
        ));
    }
    Ok(())
}

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
            let local = Client::new(services.local.clone());
            let chosen = selection.clone();
            let token = stop.clone();
            let enabled = services.relay_enabled;
            let probe = services.runtime.spawn(async move {
                if enabled {
                    prepare(&local, chosen, &token).await
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
                    } else if matches!(probe, Ok(Err(ref error)) if matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable)) {
                        "host_network_unknown"
                    } else {
                        "pairing_relay_failed"
                    });
                } else {
                    view.status = tr("pairing_preparing");
                    if enabled && matches!(selection, RelaySelection::Custom(_)) {
                        view.relay_feedback = Some("pairing_relay_ready");
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
