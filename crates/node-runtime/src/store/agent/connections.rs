//! Connection credentials stay behind the same Node workers used by desktop clients.
use super::*;
use sailry_protocol::connection::Resource;
use serde::Deserialize;

pub(crate) struct Catalog {
    pub bound: Option<Resource>,
    pub databases: Vec<database::Profile>,
    pub ssh: Vec<ssh::Profile>,
}

pub(crate) fn catalog(db: &Connection, session: SessionId) -> Result<Catalog, Fault> {
    let session = super::super::commands::session(db, session)?;
    let allowed = |target, sharing: Option<&sailry_protocol::connection::Sharing>| {
        super::super::connections::permits(
            session.config.resource,
            session.project,
            target,
            sharing,
        )
    };
    Ok(Catalog {
        bound: session.config.resource,
        databases: super::super::databases::list(db)?
            .into_iter()
            .filter(|profile| allowed(Resource::Database(profile.id), profile.sharing.as_ref()))
            .collect(),
        ssh: super::super::ssh::list(db)?
            .into_iter()
            .filter(|profile| allowed(Resource::Ssh(profile.id), profile.sharing.as_ref()))
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Catalog {
        database: Option<String>,
    },
    Query {
        read_only: bool,
        database: Option<String>,
        sql: String,
        row_limit: u32,
        timeout_ms: u64,
    },
    Run {
        command: String,
        timeout_ms: Option<u64>,
    },
    Transfer {
        path: String,
        remote_path: String,
        direction: ssh::Direction,
        timeout_ms: u64,
    },
}

impl Ingress {
    pub(crate) async fn connection_command(
        &self,
        turn: TurnId,
        target: Resource,
        action: Action,
    ) -> Result<Command, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Connection {
                turn,
                target,
                action,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn command(
    db: &Connection,
    turn: TurnId,
    target: Resource,
    action: Action,
) -> Result<Command, Fault> {
    let run = super::calls::active(db, turn)?;
    let available = catalog(db, run.session)?;
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "connection is not available to this conversation",
        )
    };
    match target {
        Resource::Database(id) => {
            let profile = available
                .databases
                .iter()
                .find(|profile| profile.id == id)
                .ok_or_else(denied)?;
            match action {
                Action::Catalog { database } => Ok(Command::BrowseDatabase {
                    profile: id,
                    expected_revision: profile.revision,
                    database,
                }),
                Action::Query {
                    database,
                    sql,
                    read_only,
                    row_limit,
                    timeout_ms,
                } => Ok(Command::QueryDatabase {
                    read_only,
                    profile: id,
                    expected_revision: profile.revision,
                    database,
                    sql,
                    row_limit,
                    timeout_ms,
                }),
                _ => Err(denied()),
            }
        }
        Resource::Ssh(id) => {
            let profile = available
                .ssh
                .iter()
                .find(|profile| profile.id == id)
                .ok_or_else(denied)?;
            match action {
                Action::Run {
                    command,
                    timeout_ms,
                } => Ok(Command::RunSsh {
                    profile: id,
                    expected_revision: profile.revision,
                    command,
                    timeout_ms: timeout_ms.unwrap_or(120_000),
                }),
                Action::Transfer {
                    path,
                    remote_path,
                    direction,
                    timeout_ms,
                } => Ok(Command::TransferSsh {
                    timeout_ms,
                    profile: id,
                    expected_revision: profile.revision,
                    transfer: ssh::Transfer {
                        worktree: run.worktree,
                        path,
                        remote_path,
                        direction,
                    },
                }),
                _ => Err(denied()),
            }
        }
    }
}
