//! Durable relay changes apply to the Node's existing endpoint after admission.
use super::{
    Ingress, Job,
    database::{Database, encode, storage_error},
    external::Completed,
};
use futures::FutureExt;
use sailry_link::{
    Admission, CancellationToken, LinkHandle, NetworkScope, RelaySelection, Response,
};
use sailry_protocol::{
    network::{Selection, Settings},
    *,
};
use serde::{Deserialize, Serialize};
use std::panic::AssertUnwindSafe;
use tokio::{
    runtime::Handle,
    sync::{mpsc, oneshot},
};
use turso::Connection;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stored {
    revision: u64,
    selection: Selection,
}

async fn stored(db: &Connection) -> Result<Option<Stored>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM network_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) async fn validate_stored(db: &Connection) -> Result<(), Fault> {
    if let Some(saved) = stored(db).await? {
        transport(&saved.selection).validate()?;
    }
    Ok(())
}

pub(super) async fn scope(db: &Connection, fallback: NetworkScope) -> Result<NetworkScope, Fault> {
    // Direct endpoints are explicit isolated/test transports, never Internet settings.
    if matches!(fallback, NetworkScope::Direct(_)) {
        return Ok(fallback);
    }
    Ok(match stored(db).await?.map(|saved| saved.selection) {
        Some(Selection::Iroh) => NetworkScope::Internet,
        Some(Selection::Custom { relays }) => {
            RelaySelection::Custom(relays.clone()).validate()?;
            NetworkScope::CustomRelays(relays)
        }
        None => fallback,
    })
}

pub(in crate::store) fn transport(value: &Selection) -> RelaySelection {
    match value {
        Selection::Iroh => RelaySelection::Default,
        Selection::Custom { relays } => RelaySelection::Custom(relays.clone()),
    }
}

fn configured(value: RelaySelection) -> Selection {
    match value {
        RelaySelection::Default => Selection::Iroh,
        RelaySelection::Custom(relays) => Selection::Custom { relays },
    }
}

async fn read(db: &Connection, link: &LinkHandle) -> Result<Settings, Fault> {
    let effective = link.relay_selection()?.map(configured);
    let saved = stored(db).await?;
    Ok(Settings {
        revision: saved.as_ref().map_or(0, |saved| saved.revision),
        selection: saved
            .map(|saved| saved.selection)
            .or_else(|| effective.clone()),
        effective,
        address: serde_json::to_string(&link.address()).map_err(storage_error)?,
    })
}

impl Ingress {
    pub(super) async fn read_network(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if request.plugin.is_some() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "network configuration requires a controller",
            ));
        }
        let storage = self.storage.get().ok_or_else(super::unavailable)?;
        let link = storage
            .network_link
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(super::unavailable)?;
        let settings = storage
            .read(move |connection| Box::pin(async move { read(connection, &link).await }))
            .await?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::NetworkSettings(settings)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}

pub(super) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
    link: LinkHandle,
    settings: Settings,
    change: Option<RelaySelection>,
}

type Rejected = Box<(Request, oneshot::Sender<Response>, Fault)>;

pub(super) async fn prepare(
    db: &mut Database,
    caller: NodeId,
    request: Request,
    reply: oneshot::Sender<Response>,
) -> Result<Execution, Rejected> {
    let prepared = (async || {
        if request.plugin.is_some() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "network configuration requires a controller",
            ));
        }
        let link = db.network_link.lock().unwrap().clone().ok_or_else(super::unavailable)?;
        let mut settings = read(&db.connection, &link).await?;
        let change = if let Command::SaveNetworkSettings {
            expected_revision,
            selection: desired,
        } = &request.command
        {
            if settings.effective.is_none() {
                return Err(Fault::invalid(
                    "direct-only endpoint has no relay transport",
                ));
            }
            if db.network_request.lock().unwrap().is_some() {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "network configuration is changing",
                ));
            }
            if settings.revision != *expected_revision {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "network configuration changed",
                ));
            }
            let change = transport(desired);
            change.validate()?;
            let revision = settings
                .revision
                .checked_add(1)
                .ok_or_else(|| Fault::invalid("network revision is exhausted"))?;
            let saved = Stored {
                revision,
                selection: desired.clone(),
            };
            db.connection.execute("INSERT INTO network_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).await.map_err(storage_error)?;
            settings.revision = revision;
            settings.selection = Some(desired.clone());
            *db.network_request.lock().unwrap() = Some(request.id);
            Some(change)
        } else {
            None
        };
        Ok((link, settings, change))
    })().await;
    match prepared {
        Ok((link, settings, change)) => Ok(Execution {
            caller,
            request,
            reply,
            link,
            settings,
            change,
        }),
        Err(error) => Err(Box::new((request, reply, error))),
    }
}

pub(super) async fn release(db: &mut Database, request: &Request) {
    if *db.network_request.lock().unwrap() == Some(request.id) {
        *db.network_request.lock().unwrap() = None;
    }
}

pub(super) struct Worker {
    pub pending: usize,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
}

impl Worker {
    pub fn new(runtime: Handle, completed: mpsc::Sender<Job>, closed: CancellationToken) -> Self {
        Self {
            pending: 0,
            runtime,
            completed,
            closed,
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        if self.pending >= super::CAPACITY || self.closed.is_cancelled() {
            return Err(Box::new(execution));
        }
        self.pending += 1;
        let completed = self.completed.clone();
        let stop = self.closed.child_token();
        self.runtime.spawn(async move {
            let operation = async {
                let mut settings = execution.settings;
                if let Some(change) = execution.change {
                    execution.link.select_relays(change, stop).await?;
                    settings.effective = execution.link.relay_selection()?.map(configured);
                    settings.address = serde_json::to_string(&execution.link.address()).map_err(storage_error)?;
                }
                Ok(Output::NetworkSettings(settings))
            };
            let result = AssertUnwindSafe(operation).catch_unwind().await.unwrap_or_else(|_| Err(Fault::new(ErrorCode::OutcomeUnknown, "network change could not be confirmed; inspect the original request before retrying")));
            let _ = completed.send(Job::Network(Box::new(Completed { caller: execution.caller, request: execution.request, result, reply: execution.reply }))).await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
