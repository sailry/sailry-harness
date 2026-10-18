//! Installation recovery queries admitted work and never repeats an installation.
use super::*;
use sailry_link::{EndpointAddr, RelaySelection};

#[derive(Clone)]
pub(super) enum Recovery {
    Receipt,
    Network {
        node: NodeId,
        selection: sailry_protocol::network::Selection,
    },
    Partial,
}

enum Checked {
    Receipt(RequestOutcome),
    Network(NodeId),
}

impl Installer {
    pub(super) fn unconfirmed(&mut self, error: Fault, cx: &mut Context<Self>) {
        self.error = Some(error);
        self.form = true;
        self.key = None;
        self.changed = false;
        // Keep credentials and the target draft locked until the admitted result is known.
        self.editor.update(cx, |editor, cx| {
            editor.locked = true;
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        let Some(recovery) = self.recovery.clone() else {
            return;
        };
        let request = self.request.clone();
        let client = self.client.clone();
        let services = self.services.clone();
        self.checking = true;
        self.error = None;
        let job = self.runtime.spawn(async move {
            match recovery {
                Recovery::Receipt => {
                    let request =
                        request.ok_or_else(|| Fault::invalid("installation request missing"))?;
                    client.outcome(&request).await.map(Checked::Receipt)
                }
                Recovery::Network { node, selection } => {
                    confirm(&services, node, &selection).await?;
                    Ok(Checked::Network(node))
                }
                Recovery::Partial => Err(Fault::invalid("installation cannot be confirmed")),
            }
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::Unavailable,
                    "installation status unavailable",
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.checking = false;
                match result {
                    Ok(Checked::Network(node)) => this.complete(
                        Ok(Output::SshOutcome(Outcome::HostInstalled { node })),
                        window,
                        cx,
                    ),
                    Ok(Checked::Receipt(RequestOutcome::Completed(result))) => match *result {
                        Err(error) if error.code == ErrorCode::OutcomeUnknown => {
                            this.recovery = Some(Recovery::Partial);
                            this.request = None;
                            this.unconfirmed(error, cx);
                        }
                        Err(error) => {
                            this.request = None;
                            this.recovery = None;
                            this.failed(error, cx);
                        }
                        Ok(output) => this.complete(Ok(output), window, cx),
                    },
                    Ok(Checked::Receipt(RequestOutcome::NotAdmitted)) => {
                        this.request = None;
                        this.recovery = None;
                        this.failed(Fault::invalid("installation request was not admitted"), cx);
                    }
                    Ok(Checked::Receipt(RequestOutcome::Unknown)) => {
                        this.recovery = Some(Recovery::Partial);
                        this.request = None;
                        this.unconfirmed(
                            Fault::new(ErrorCode::OutcomeUnknown, "installation result is unknown"),
                            cx,
                        );
                    }
                    Ok(Checked::Receipt(RequestOutcome::Admitted)) => {
                        this.unconfirmed(
                            Fault::new(ErrorCode::OutcomeUnknown, "installation is still running"),
                            cx,
                        );
                    }
                    Err(error) => this.unconfirmed(error, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

async fn confirm(
    services: &Services,
    node: NodeId,
    selected: &sailry_protocol::network::Selection,
) -> Result<(), Fault> {
    let previous = services
        .link
        .peers()
        .await?
        .into_iter()
        .find(|address| address.id.as_bytes() == &node.0)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "installed Host is not paired"))?;
    let client = Client::new(services.link.remote(previous.clone()));
    let request = client.prepare(Command::ReadNetworkSettings);
    let output = match client.execute(request.clone()).await {
        Ok(output) => output,
        Err(_) => {
            let transport = match selected {
                sailry_protocol::network::Selection::Iroh => RelaySelection::Default,
                sailry_protocol::network::Selection::Custom { relays } => {
                    RelaySelection::Custom(relays.clone())
                }
            };
            services
                .link
                .update_peer(services.link.candidate_address(&previous, &transport)?)
                .await?;
            client.execute(request).await?
        }
    };
    let Output::NetworkSettings(settings) = output else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if settings.revision == 0
        || settings.selection.as_ref() != Some(selected)
        || settings.effective.as_ref() != Some(selected)
    {
        return Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "Host network setup is unconfirmed",
        ));
    }
    let address: EndpointAddr = serde_json::from_str(&settings.address)
        .map_err(|_| Fault::invalid("invalid Host network address"))?;
    if address.id.as_bytes() != &node.0 {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "Host network identity does not match",
        ));
    }
    services.link.update_peer(address).await
}
