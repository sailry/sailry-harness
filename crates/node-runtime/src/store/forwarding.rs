//! Session configuration crosses the existing paired Link without exposing secrets to UI.
use std::{panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;
use sailry_link::{CancellationToken, LinkHandle, Response};
use sailry_protocol::*;
use tokio::{
    runtime::Handle,
    sync::{mpsc, oneshot},
};

use super::{CAPACITY, Job, database::Database, external::Completed};

pub(super) struct Transfer {
    pub caller: NodeId,
    pub request: Request,
    pub forwarded: Box<Request>,
    pub reply: oneshot::Sender<Response>,
}

pub(super) fn prepare(db: &Database, caller: NodeId, request: &Request) -> Result<Request, Fault> {
    let Command::CreateSessionAt {
        target,
        project,
        worktree,
        config,
        provider_revision,
    } = &request.command
    else {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "configuration forwarding expected",
        ));
    };
    if caller != db.node || *target == db.node {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "only the local controller can forward configuration to another Node",
        ));
    }
    let mut provider = super::agent::providers::get(&db.connection, config.provider)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "source provider does not exist"))?;
    if provider.revision != *provider_revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "source provider revision changed",
        ));
    }
    super::commands::validate_config(&db.connection, db.node, config)?;
    // The explicit selection is authoritative, including an absent credential.
    provider.credential = config.credential.clone();
    if provider.authentication == Authentication::Host {
        crate::providers::login::transferable(provider.authentication)?;
    }
    let (secret, expires_at_ms) = if let Some(reference) = &config.credential {
        crate::providers::login::transferable(provider.authentication)?;
        let secret = db.resolve_credential(reference, config.provider, provider.authentication)?;
        let metadata = super::providers::authentication::get(&db.connection, reference.id)?
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "source credential does not exist"))?;
        (Some(secret), metadata.expires_at_ms)
    } else {
        (None, None)
    };
    let roles = super::sessions::roles::capture(
        &db.connection,
        db.node,
        super::roles::list(&db.connection)?,
    )?;
    let role_credentials = super::sessions::roles::export(db, &roles)?;
    Ok(Request {
        version: request.version,
        id: request.id,
        target: *target,
        plugin: None,
        command: Command::ImportSession(Box::new(SessionImport {
            source: db.node,
            project: *project,
            worktree: *worktree,
            config: config.as_ref().clone(),
            provider,
            secret,
            expires_at_ms,
            roles,
            role_credentials,
        })),
    })
}

pub(super) struct Forwarder {
    pub link: Option<LinkHandle>,
    pub pending: usize,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
}

impl Forwarder {
    pub fn new(runtime: Handle, completed: mpsc::Sender<Job>, closed: CancellationToken) -> Self {
        Self {
            link: None,
            pending: 0,
            runtime,
            completed,
            closed,
        }
    }

    pub fn submit(&mut self, transfer: Transfer) -> Result<(), Box<Transfer>> {
        if self.pending >= CAPACITY || self.link.is_none() || self.closed.is_cancelled() {
            return Err(Box::new(transfer));
        }
        let link = self.link.clone().expect("forwarding link was checked");
        let closed = self.closed.clone();
        let completed = self.completed.clone();
        self.pending += 1;
        self.runtime.spawn(async move {
            let operation = async {
                tokio::select! {
                    biased;
                    _ = closed.cancelled() => Err(unknown()),
                    result = tokio::time::timeout(Duration::from_secs(30), send(link, *transfer.forwarded)) => {
                        result.unwrap_or_else(|_| Err(unknown()))
                    }
                }
            };
            let result = AssertUnwindSafe(operation).catch_unwind().await.unwrap_or_else(|_| Err(unknown()));
            let _ = completed.send(Job::Transferred(Box::new(Completed {
                caller: transfer.caller,
                request: transfer.request,
                result,
                reply: transfer.reply,
            }))).await;
        });
        Ok(())
    }
}

async fn send(link: LinkHandle, request: Request) -> Response {
    let address = link
        .peers()
        .await?
        .into_iter()
        .find(|address| NodeId(*address.id.as_bytes()) == request.target)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "target Node is not paired"))?;
    // Once dispatch begins, loss of the response cannot establish non-admission.
    let admission = link
        .remote(address)
        .dispatch(request)
        .await
        .map_err(|_| unknown())?;
    admission.completion.await.map_err(|_| unknown())?
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "remote session creation could not be confirmed; automatic replay is disabled",
    )
}
