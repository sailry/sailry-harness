//! Host network edits use the selected Node's durable service and existing identity.
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{dialog::DialogFooter, spinner::Spinner};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::Client;
use sailry_link::EndpointAddr;
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output, Request, RequestOutcome,
    network::{Selection, Settings},
};
use std::sync::Arc;

pub(crate) fn open(target: NodeId, window: &mut Window, cx: &mut App) {
    let Some(services) = cx.try_global::<Services>().cloned() else {
        return;
    };
    let form = cx.new(|cx| {
        let (select, address) = relay::fields_for(None, window, cx);
        cx.observe(&select, |_, _, cx| cx.notify()).detach();
        cx.observe(&address, |_, _, cx| cx.notify()).detach();
        crate::feedback::observe(window, cx, |form: &Editor, _| {
            form.error.into_iter().collect()
        });
        Editor {
            services,
            target,
            client: None,
            saved: None,
            select,
            address,
            pending: false,
            request: None,
            closed: false,
            error: None,
        }
    });
    form.update(cx, |form, cx| form.load(window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let closing = form.clone();
        dialog
            .form_title(tr("host_network_edit"), cx)
            .w(px(400.).min(window.viewport_size().width - px(48.)))
            .overlay_closable(false)
            .on_close(move |_, _, cx| closing.update(cx, |form, _| form.closed = true))
            .child(form.clone())
    });
}

struct Editor {
    services: Services,
    target: NodeId,
    client: Option<Arc<Client>>,
    saved: Option<Settings>,
    select: Entity<SelectState<Vec<relay::Network>>>,
    address: Entity<InputState>,
    pending: bool,
    request: Option<Request>,
    closed: bool,
    error: Option<&'static str>,
}

pub(super) fn desired(selection: RelaySelection) -> Selection {
    match selection {
        RelaySelection::Default => Selection::Iroh,
        RelaySelection::Custom(relays) => Selection::Custom { relays },
    }
}

pub(super) fn transport(selection: &Selection) -> RelaySelection {
    match selection {
        Selection::Iroh => RelaySelection::Default,
        Selection::Custom { relays } => RelaySelection::Custom(relays.clone()),
    }
}

pub(super) fn preserve_relays(selected: Selection, current: &Settings) -> Selection {
    // Preserve an existing relay set when its displayed first address is unchanged.
    if let (Selection::Custom { relays }, Some(Selection::Custom { relays: previous })) =
        (&selected, &current.effective)
        && relays.first() == previous.first()
    {
        return Selection::Custom {
            relays: previous.clone(),
        };
    }
    selected
}

async fn read(client: &Client) -> Result<Settings, Fault> {
    match client
        .execute(client.prepare(Command::ReadNetworkSettings))
        .await?
    {
        Output::NetworkSettings(settings) => Ok(settings),
        _ => Err(Fault::invalid("network settings response expected")),
    }
}

async fn client(services: &Services, target: NodeId) -> Result<Arc<Client>, Fault> {
    if target == services.local.target() {
        return Ok(Arc::new(Client::new(services.local.clone())));
    }
    let address = services
        .link
        .peers()
        .await?
        .into_iter()
        .find(|address| address.id.as_bytes() == &target.0)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "host is not paired"))?;
    Ok(Arc::new(Client::new(services.link.remote(address))))
}

fn address(settings: &Settings, target: NodeId) -> Result<EndpointAddr, Fault> {
    let address: EndpointAddr = serde_json::from_str(&settings.address)
        .map_err(|_| Fault::invalid("invalid network address"))?;
    if address.id.as_bytes() != &target.0 {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "network address belongs to another Node",
        ));
    }
    Ok(address)
}

struct ChangeError {
    error: Fault,
    uncertain: bool,
}

fn rejected(error: Fault) -> ChangeError {
    ChangeError {
        error,
        uncertain: false,
    }
}

fn unknown(_: impl std::fmt::Display) -> ChangeError {
    ChangeError {
        error: Fault::new(
            ErrorCode::OutcomeUnknown,
            "host network change could not be confirmed",
        ),
        uncertain: true,
    }
}

fn resolved(outcome: RequestOutcome) -> Result<Output, ChangeError> {
    match outcome {
        RequestOutcome::Completed(result) => (*result).map_err(|error| {
            if error.code == ErrorCode::OutcomeUnknown {
                unknown(error)
            } else {
                rejected(error)
            }
        }),
        RequestOutcome::NotAdmitted => {
            Err(rejected(Fault::invalid("network change was not admitted")))
        }
        RequestOutcome::Admitted | RequestOutcome::Unknown => {
            Err(unknown("network change remains unresolved"))
        }
    }
}

async fn recover(
    services: &Services,
    client: &Client,
    request: &Request,
    previous: &Settings,
    selected: &Selection,
) -> Result<Output, ChangeError> {
    let mut outcome = client.outcome(request).await;
    if outcome.is_err() && client.target() != services.local.target() {
        let previous = address(previous, client.target()).map_err(unknown)?;
        let candidate = services
            .link
            .candidate_address(&previous, &transport(selected))
            .map_err(unknown)?;
        services
            .link
            .update_peer(candidate)
            .await
            .map_err(unknown)?;
        outcome = client.outcome(request).await;
    }
    resolved(outcome.map_err(unknown)?)
}

/// A lost response is queried through durable admission, never automatically replayed.
async fn apply(
    services: &Services,
    client: &Client,
    request: Request,
    previous: &Settings,
    selected: &Selection,
    inspect: bool,
) -> Result<Settings, ChangeError> {
    let result = if inspect {
        recover(services, client, &request, previous, selected).await?
    } else {
        match client.execute(request.clone()).await {
            Ok(output) => output,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                ) =>
            {
                recover(services, client, &request, previous, selected).await?
            }
            Err(error) => return Err(rejected(error)),
        }
    };
    let settings = match result {
        Output::NetworkSettings(settings) => settings,
        _ => return Err(unknown("network settings response expected")),
    };
    if client.target() != services.local.target() {
        services
            .link
            .update_peer(address(&settings, client.target()).map_err(unknown)?)
            .await
            .map_err(unknown)?;
    }
    let actual = read(client).await.map_err(unknown)?;
    if actual.effective.as_ref() != Some(selected) || actual.selection.as_ref() != Some(selected) {
        if actual.revision > settings.revision {
            return Err(rejected(Fault::new(
                ErrorCode::RevisionConflict,
                "host network settings changed",
            )));
        }
        return Err(unknown("host network change could not be confirmed"));
    }
    Ok(actual)
}

impl Editor {
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.error = None;
        let services = self.services.clone();
        let target = self.target;
        let job = services.runtime.clone().spawn(async move {
            let client = client(&services, target).await?;
            let settings = read(&client).await?;
            Ok::<_, Fault>((client, settings))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |form, window, cx| {
                form.pending = false;
                if form.closed {
                    return;
                }
                match result {
                    Ok(Ok((client, settings))) => {
                        let selected = settings.effective.as_ref().or(settings.selection.as_ref());
                        if let Some(selected) = selected {
                            let custom = matches!(selected, Selection::Custom { .. });
                            form.select.update(cx, |state, cx| {
                                state.set_selected_index(
                                    Some(IndexPath::new(usize::from(custom))),
                                    window,
                                    cx,
                                );
                            });
                            let value = match selected {
                                Selection::Iroh => String::new(),
                                Selection::Custom { relays } => {
                                    relays.first().cloned().unwrap_or_default()
                                }
                            };
                            form.address
                                .update(cx, |input, cx| input.set_value(value, window, cx));
                        }
                        form.client = Some(client);
                        if settings.effective.is_none() {
                            form.error = Some("host_network_unavailable");
                        }
                        form.saved = Some(settings);
                    }
                    _ => form.error = Some("host_network_load_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let (Some(client), Some(saved)) = (self.client.clone(), self.saved.clone()) else {
            self.load(window, cx);
            return;
        };
        let Some(selected) = relay::selection(&self.select, &self.address, cx) else {
            self.error = Some("pairing_relay_invalid");
            cx.notify();
            return;
        };
        if selected.validate().is_err() {
            self.error = Some("pairing_relay_invalid");
            cx.notify();
            return;
        }
        let selected = preserve_relays(desired(selected), &saved);
        let inspect = self.request.is_some();
        let request = self
            .request
            .get_or_insert_with(|| {
                client.prepare(Command::SaveNetworkSettings {
                    expected_revision: saved.revision,
                    selection: selected.clone(),
                })
            })
            .clone();
        let selected = match &request.command {
            Command::SaveNetworkSettings { selection, .. } => selection.clone(),
            _ => return,
        };
        let services = self.services.clone();
        self.pending = true;
        self.error = None;
        let job = services.runtime.clone().spawn(async move {
            let result = apply(&services, &client, request, &saved, &selected, inspect).await;
            let refreshed = if result.is_err() {
                read(&client).await.ok()
            } else {
                None
            };
            (result, refreshed)
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |form, window, cx| {
                form.pending = false;
                if form.closed {
                    return;
                }
                match result {
                    Ok((Ok(_), _)) => {
                        form.closed = true;
                        window.push_notification(
                            notification::Notification::success(tr("host_network_saved")),
                            cx,
                        );
                        window.close_dialog(cx);
                    }
                    Ok((Err(error), refreshed)) => {
                        if let Some(settings) = refreshed {
                            form.saved = Some(settings);
                        }
                        form.error = Some(if error.uncertain {
                            "host_network_unknown"
                        } else if error.error.code == ErrorCode::RevisionConflict {
                            "host_network_conflict"
                        } else {
                            "host_network_failed"
                        });
                        if !error.uncertain {
                            form.request = None;
                        }
                    }
                    _ => form.error = Some("host_network_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editable = self
            .saved
            .as_ref()
            .is_some_and(|settings| settings.effective.is_some());
        v_flex()
            .gap_4()
            .when(self.pending && self.saved.is_none(), |body| {
                body.child(Spinner::new())
            })
            .child(relay::controls(
                &self.select,
                &self.address,
                self.pending || self.request.is_some() || !editable,
                cx,
            ))
            .child(
                DialogFooter::new()
                    .child(
                        Button::new("host-network-cancel")
                            .label(tr("settings_cancel"))
                            .disabled(self.pending)
                            .on_click(cx.listener(|form, _, window, cx| {
                                form.closed = true;
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("host-network-save")
                            .primary()
                            .label(tr(if self.request.is_some() || self.saved.is_none() {
                                "settings_retry"
                            } else {
                                "settings_save"
                            }))
                            .loading(self.pending)
                            .disabled(self.pending || self.saved.is_some() && !editable)
                            .on_click(cx.listener(|form, _, window, cx| form.save(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{preserve_relays, resolved};
    use sailry_protocol::{
        ErrorCode, Fault, RequestOutcome,
        network::{Selection, Settings},
    };

    #[test]
    fn retains_undisplayed_relays_until_address_changes() {
        let previous = Selection::Custom {
            relays: vec![
                "https://first.example".into(),
                "https://second.example".into(),
            ],
        };
        let settings = Settings {
            revision: 1,
            selection: Some(previous.clone()),
            effective: Some(previous.clone()),
            address: String::new(),
        };
        assert_eq!(
            preserve_relays(
                Selection::Custom {
                    relays: vec!["https://first.example".into()]
                },
                &settings
            ),
            previous
        );
        let changed = Selection::Custom {
            relays: vec!["https://changed.example".into()],
        };
        assert_eq!(preserve_relays(changed.clone(), &settings), changed);
        assert_eq!(preserve_relays(Selection::Iroh, &settings), Selection::Iroh);
    }

    #[test]
    fn distinguishes_final_failure_from_uncertain_admission() {
        for outcome in [
            RequestOutcome::Admitted,
            RequestOutcome::Unknown,
            RequestOutcome::Completed(Box::new(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Fixture uncertainty",
            )))),
        ] {
            assert!(resolved(outcome).unwrap_err().uncertain);
        }
        assert!(!resolved(RequestOutcome::NotAdmitted).unwrap_err().uncertain);
        let error = resolved(RequestOutcome::Completed(Box::new(Err(Fault::new(
            ErrorCode::Unavailable,
            "Fixture relay probe failed",
        )))))
        .unwrap_err();
        assert!(!error.uncertain);
        assert_eq!(error.error.code, ErrorCode::Unavailable);
    }
}
