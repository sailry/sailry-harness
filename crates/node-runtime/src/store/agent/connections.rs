//! Connection credentials stay behind the same Node workers used by desktop clients.
use super::approvals::Prepared;
use super::*;
use sailry_protocol::connection::Resource;
mod management;
pub(crate) use management::{DatabaseInput, SshInput};

pub(crate) struct Catalog {
    pub bound: Option<Resource>,
    pub databases: Vec<database::Profile>,
    pub ssh: Vec<ssh::Profile>,
}

impl Catalog {
    pub(crate) fn resources(&self) -> Vec<Resource> {
        self.databases
            .iter()
            .map(|profile| Resource::Database(profile.id))
            .chain(self.ssh.iter().map(|profile| Resource::Ssh(profile.id)))
            .collect()
    }
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

pub(crate) enum Action {
    ListDatabases,
    ListSsh,
    SaveDatabase(DatabaseInput),
    SaveSsh(SshInput),
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
        package: plugin::Reference,
        target: Option<Resource>,
        action: Action,
        captured: Vec<Resource>,
    ) -> Result<Command, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Connection {
                turn,
                package,
                target,
                action,
                captured,
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
    package: &plugin::Reference,
    target: Option<Resource>,
    action: Action,
    captured: &[Resource],
) -> Result<Command, Fault> {
    let run = super::calls::active(db, turn)?;
    match action {
        Action::ListDatabases => return Ok(Command::ListDatabases),
        Action::ListSsh => return Ok(Command::ListSsh),
        Action::SaveDatabase(input) => {
            return management::database(db, turn, package, captured, input);
        }
        Action::SaveSsh(input) => return management::ssh(db, turn, package, captured, input),
        _ => {}
    }
    let target = target.ok_or_else(|| Fault::invalid("connection target is required"))?;
    check(db, turn, package, target, captured)?;
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "connection is not available to this conversation",
        )
    };
    match target {
        Resource::Database(id) => {
            let profile = super::super::databases::read(db, id)?.ok_or_else(denied)?;
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
            let profile = super::super::ssh::read(db, id)?.ok_or_else(denied)?;
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

pub(in crate::store) fn created(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    target: Resource,
) -> Result<bool, Fault> {
    super::calls::active(db, turn)?;
    if super::permissions::config(db, turn)?.resource.is_some() {
        return Ok(false);
    }
    let mut query = db.prepare("SELECT a.prepared,r.result,a.binding FROM agent_approvals a JOIN turns t ON t.id=a.turn JOIN requests r ON r.id=a.request AND r.caller=t.caller WHERE a.turn=?1 AND a.claimed=1 AND json_extract(a.body,'$.state')='approved' AND r.status='completed'").map_err(storage_error)?;
    let records = query
        .query_map([turn.to_string()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(storage_error)?;
    for record in records {
        let (prepared_body, result_body, binding_body) = record.map_err(storage_error)?;
        let binding: Option<ApprovalBinding> =
            serde_json::from_slice(&binding_body).map_err(storage_error)?;
        if !binding.is_some_and(|binding| binding.owner == *package) {
            continue;
        }
        let prepared: Option<Prepared> =
            serde_json::from_slice(&prepared_body).map_err(storage_error)?;
        let result = super::super::outcomes::decode(&result_body)?;
        let matched = match (target, prepared.map(|p| p.command), result) {
            (
                Resource::Database(id),
                Some(Command::SaveDatabase {
                    profile,
                    expected_revision: 0,
                    ..
                }),
                Ok(Output::DatabaseProfile(saved)),
            ) => id == profile.id && saved.id == id && saved.revision == 1,
            (
                Resource::Ssh(id),
                Some(Command::SaveSsh {
                    profile,
                    expected_revision: 0,
                    ..
                }),
                Ok(Output::SshProfile(saved)),
            ) => id == profile.id && saved.id == id && saved.revision == 1,
            _ => false,
        };
        if matched {
            return Ok(true);
        }
    }
    Ok(false)
}

fn check(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    target: Resource,
    captured: &[Resource],
) -> Result<(), Fault> {
    let run = super::calls::active(db, turn)?;
    if created(db, turn, package, target)? {
        return Ok(());
    }
    let available = catalog(db, run.session)?;
    let permitted = match target {
        Resource::Database(id) => available.databases.iter().any(|p| p.id == id),
        Resource::Ssh(id) => available.ssh.iter().any(|p| p.id == id),
    };
    if captured.contains(&target) && permitted {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "connection is not available to this turn",
        ))
    }
}

impl Ingress {
    pub(crate) async fn connection_status(
        &self,
        turn: TurnId,
        package: plugin::Reference,
        targets: Vec<Resource>,
        captured: Vec<Resource>,
    ) -> Result<std::collections::BTreeMap<Resource, bool>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ConnectionStatus {
                turn,
                package,
                targets,
                captured,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
    pub(crate) async fn connection_names(
        &self,
        turn: TurnId,
        entries: Vec<(plugin::Reference, Resource)>,
        captured: Vec<Resource>,
    ) -> Result<Vec<Option<String>>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ConnectionNames {
                turn,
                entries,
                captured,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
    pub(crate) async fn connection_trust(
        &self,
        turn: TurnId,
        package: plugin::Reference,
        profile: SshId,
        revision: u64,
        key: ssh::HostKey,
        changed: bool,
        captured: Vec<Resource>,
    ) -> Result<ssh::Trust, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ConnectionTrust {
                turn,
                package,
                profile,
                revision,
                key,
                changed,
                captured,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn names(
    db: &Connection,
    turn: TurnId,
    entries: &[(plugin::Reference, Resource)],
    captured: &[Resource],
) -> Result<Vec<Option<String>>, Fault> {
    entries
        .iter()
        .map(|(package, target)| {
            if !created(db, turn, package, *target)? {
                return Ok(None);
            }
            check(db, turn, package, *target, captured)?;
            let name = match target {
                Resource::Database(id) => {
                    super::super::databases::read(db, *id)?.map(|profile| profile.name)
                }
                Resource::Ssh(id) => super::super::ssh::read(db, *id)?.map(|profile| profile.name),
            };
            Ok(name.filter(|name| !name.trim().is_empty()))
        })
        .collect()
}

pub(super) fn status(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    targets: &[Resource],
    captured: &[Resource],
) -> Result<std::collections::BTreeMap<Resource, bool>, Fault> {
    let mut result = std::collections::BTreeMap::new();
    for target in targets {
        match check(db, turn, package, *target, captured) {
            Ok(()) => {}
            Err(error) if error.code == ErrorCode::PermissionDenied => continue,
            Err(error) => return Err(error),
        }
        let missing = if let Resource::Database(id) = target {
            let profile = super::super::databases::read(db, *id)?.ok_or_else(|| {
                Fault::new(ErrorCode::NotFound, "database connection does not exist")
            })?;
            if matches!(profile.connection, database::Connection::Sqlite { .. }) {
                false
            } else {
                db.query_row(
                    "SELECT length(password)=0 FROM database_profiles WHERE id=?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?
            }
        } else {
            false
        };
        result.insert(*target, missing);
    }
    Ok(result)
}

pub(super) fn trust(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    profile: SshId,
    revision: u64,
    key: ssh::HostKey,
    changed: bool,
    captured: &[Resource],
) -> Result<ssh::Trust, Fault> {
    check(db, turn, package, Resource::Ssh(profile), captured)?;
    let profile = super::super::ssh::read(db, profile)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "SSH connection does not exist"))?;
    super::super::commands::check_revision(profile.revision, revision)?;
    Ok(ssh::Trust {
        profile: profile.id,
        expected_revision: revision,
        host: profile.host,
        port: profile.port,
        key,
        changed,
    })
}

pub(super) fn validate_trust(
    db: &Connection,
    turn: TurnId,
    binding: Option<&ApprovalBinding>,
    trust: &ssh::Trust,
) -> Result<(), Fault> {
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "SSH host key consent is outside the captured operation",
        )
    };
    let binding = binding
        .filter(|b| {
            matches!(
                b.operation,
                Some(
                    sailry_protocol::tool::Operation::RunSsh
                        | sailry_protocol::tool::Operation::TransferSsh
                )
            )
        })
        .ok_or_else(denied)?;
    if !super::plugins::read(db, turn)?.contains(&binding.owner) {
        return Err(denied());
    }
    let profile = super::super::ssh::read(db, trust.profile)?.ok_or_else(denied)?;
    if profile.revision != trust.expected_revision
        || profile.host != trust.host
        || profile.port != trust.port
        || trust.changed != profile.host_key.is_some()
        || profile.host_key.as_ref() == Some(&trust.key)
    {
        return Err(denied());
    }
    Ok(())
}

pub(in crate::store) fn owner(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    super::calls::active(db, turn)
}
pub(in crate::store) fn bound(db: &Connection, turn: TurnId) -> Result<Option<Resource>, Fault> {
    Ok(super::permissions::config(db, turn)?.resource)
}
