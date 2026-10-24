//! Exact durable admission retains a digest without duplicating credentials.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::store) struct Prepared {
    pub(in crate::store) command: Option<Command>,
    digest: Option<String>,
    #[serde(default)]
    pub(super) targets: Vec<(sailry_protocol::connection::Resource, u64)>,
}

impl Prepared {
    pub(in crate::store) fn new(command: &Command) -> Result<Option<Self>, Fault> {
        if !matches!(
            command,
            Command::BrowseDatabase { .. }
                | Command::QueryDatabase { .. }
                | Command::RunSsh { .. }
                | Command::TransferSsh { .. }
                | Command::SaveDatabase { .. }
                | Command::InstallLocalModel { .. }
                | Command::DeleteLocalModel { .. }
                | Command::SaveSsh { .. }
                | Command::PutRole { .. }
                | Command::RemoveRole { .. }
                | Command::SetSessionRoles { .. }
        ) {
            return Ok(None);
        }
        let digest = blake3::hash(&encode(command)?).to_hex().to_string();
        let mut command = command.clone();
        match &mut command {
            Command::SaveDatabase { password, .. } => *password = None,
            Command::SaveSsh { credential, .. } => *credential = None,
            _ => {}
        }
        Ok(Some(Self {
            command: Some(command),
            digest: Some(digest),
            targets: vec![],
        }))
    }

    pub(in crate::store) fn matches(&self, command: &Command) -> Result<bool, Fault> {
        Ok(self.digest.as_deref() == Some(blake3::hash(&encode(command)?).to_hex().as_str()))
    }
}

/// Script argument transformations stay behind their existing approval. Freeze
/// available target revisions first; never evaluate package code inside Store.
pub(super) async fn baseline(
    db: &Connection,
    turn: TurnId,
    binding: Option<&Binding>,
) -> Result<Option<Prepared>, Fault> {
    use sailry_protocol::connection::Resource;
    use sailry_protocol::tool::Operation;
    let Some(binding) = binding else {
        return Ok(None);
    };
    let connections = |operation| {
        matches!(
            operation,
            Operation::ExecuteDatabase | Operation::RunSsh | Operation::TransferSsh
        )
    };
    let applicable = if let Some(operation) = binding.operation {
        connections(operation)
    } else {
        super::super::plugins::packages(db, std::slice::from_ref(&binding.owner))
            .await?
            .iter()
            .any(|package| {
                package
                    .extension
                    .iter()
                    .flat_map(|extension| &extension.tools)
                    .any(|tool| {
                        crate::plugins::tools::alias(&package.summary.name, &tool.name)
                            == binding.tool
                            && tool
                                .handler
                                .as_ref()
                                .and_then(|handler| handler.flow.as_ref())
                                .is_some_and(|flow| {
                                    flow.operations.iter().copied().any(connections)
                                })
                    })
            })
    };
    if !applicable {
        return Ok(None);
    }
    let run = super::super::calls::active(db, turn).await?;
    let catalog = super::super::connections::catalog(db, run.session).await?;
    let targets = catalog
        .databases
        .into_iter()
        .map(|profile| (Resource::Database(profile.id), profile.revision))
        .chain(
            catalog
                .ssh
                .into_iter()
                .map(|profile| (Resource::Ssh(profile.id), profile.revision)),
        )
        .collect();
    Ok(Some(Prepared {
        command: None,
        digest: None,
        targets,
    }))
}

pub(super) async fn check_connection(
    db: &Connection,
    approval: &Approval,
    command: Option<&Command>,
) -> Result<(), Fault> {
    use sailry_protocol::connection::Resource;
    let target = match command {
        Some(
            Command::BrowseDatabase {
                profile,
                expected_revision,
                ..
            }
            | Command::QueryDatabase {
                profile,
                expected_revision,
                ..
            },
        ) => Some((Resource::Database(*profile), *expected_revision)),
        Some(
            Command::RunSsh {
                profile,
                expected_revision,
                ..
            }
            | Command::TransferSsh {
                profile,
                expected_revision,
                ..
            },
        ) => Some((Resource::Ssh(*profile), *expected_revision)),
        _ => return Ok(()),
    };
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT prepared FROM agent_approvals WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([approval.id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let frozen: Option<Prepared> = serde_json::from_slice(&body).map_err(storage_error)?;
    if let (Some(frozen), Some(command)) = (frozen, command)
        && (frozen.matches(command)?
            || target.is_some_and(|target| frozen.targets.contains(&target)))
    {
        return Ok(());
    }
    Err(Fault::new(
        ErrorCode::Conflict,
        "connection changed after tool approval was requested",
    ))
}
