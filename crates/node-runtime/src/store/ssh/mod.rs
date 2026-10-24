//! Connection ownership follows sailry-code 67ae9fa0 sailry-connections.
//! Profiles and credentials now share the Node transaction and admission owner.
use super::database::{encode, storage_error};
use sailry_protocol::{ssh::*, *};
use turso::{Connection, params};

pub(super) mod worker;

pub(super) async fn list(db: &Connection) -> Result<Vec<Profile>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM ssh_profiles ORDER BY id")
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
    .collect()
}

pub(super) async fn read(db: &Connection, id: SshId) -> Result<Option<Profile>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM ssh_profiles WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
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

async fn current(db: &Connection, id: SshId, revision: u64) -> Result<Profile, Fault> {
    let profile = read(db, id).await?.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "SSH profile does not exist on this Node",
        )
    })?;
    super::commands::check_revision(profile.revision, revision)?;
    Ok(profile)
}

pub(super) async fn execute(
    db: &Connection,
    command: &Command,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::ListSsh => Ok((Output::SshProfiles(list(db).await?), None)),
        Command::SaveSsh {
            profile,
            expected_revision,
            credential,
        } => {
            let previous = read(db, profile.id).await?;
            super::commands::check_revision(
                previous.as_ref().map_or(0, |p| p.revision),
                *expected_revision,
            )?;
            super::connections::validate(db, profile.sharing.as_ref()).await?;
            let mut profile = profile.clone();
            profile.name = profile.name.trim().into();
            profile.host = profile.host.trim().into();
            profile.username = profile.username.trim().into();
            if profile.name.is_empty()
                || profile.name.len() > 256
                || profile.host.is_empty()
                || profile.host.len() > 253
                || profile
                    .host
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())
                || profile.port == 0
                || profile.username.is_empty()
                || profile.username.len() > 256
                || profile.username.chars().any(char::is_control)
            {
                return Err(sailry_protocol::Fault::invalid("invalid SSH profile"));
            }
            if profile.host_key != previous.as_ref().and_then(|p| p.host_key.clone()) {
                return Err(sailry_protocol::Fault::invalid(
                    "host keys require explicit confirmation",
                ));
            }
            if previous
                .as_ref()
                .is_some_and(|p| p.host != profile.host || p.port != profile.port)
            {
                profile.host_key = None;
            }
            let credential = if let Some(credential) = credential {
                if credential.authentication() != profile.authentication {
                    return Err(sailry_protocol::Fault::invalid(
                        "SSH credential authentication does not match",
                    ));
                }
                let body = zeroize::Zeroizing::new(
                    serde_json::to_string(credential).map_err(storage_error)?,
                );
                if body.len() > 65536 {
                    return Err(sailry_protocol::Fault::invalid(
                        "SSH credential exceeds the size limit",
                    ));
                }
                Some(body)
            } else {
                if previous
                    .as_ref()
                    .is_none_or(|p| p.authentication != profile.authentication)
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "SSH credential is required",
                    ));
                }
                None
            };
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or_else(|| sailry_protocol::Fault::invalid("SSH profile revision exhausted"))?;
            if previous.is_none() {
                db.execute(
                    "INSERT INTO ssh_profiles(id,body,credential) VALUES(?1,?2,?3)",
                    params![
                        profile.id.to_string(),
                        encode(&profile)?,
                        credential.as_deref().expect("new credential").as_str()
                    ],
                )
                .await
                .map_err(storage_error)?;
            } else {
                db.execute("UPDATE ssh_profiles SET body=?2,credential=COALESCE(?3,credential) WHERE id=?1",
                    params![profile.id.to_string(), encode(&profile)?, credential.as_deref().map(String::as_str)]).await.map_err(storage_error)?;
            }
            Ok((
                Output::SshProfile(profile.clone()),
                Some(Event::SshChanged(profile)),
            ))
        }
        Command::TrustSsh {
            profile,
            expected_revision,
            key,
        } => {
            let mut profile = current(db, *profile, *expected_revision).await?;
            use base64::Engine;
            let digest = key.fingerprint.strip_prefix("SHA256:").and_then(|value| {
                base64::engine::general_purpose::STANDARD_NO_PAD
                    .decode(value)
                    .ok()
            });
            if key.algorithm.is_empty()
                || key.algorithm.len() > 128
                || digest.is_none_or(|digest| digest.len() != 32)
            {
                return Err(sailry_protocol::Fault::invalid("invalid SSH host key"));
            }
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or_else(|| sailry_protocol::Fault::invalid("SSH profile revision exhausted"))?;
            profile.host_key = Some(key.clone());
            db.execute(
                "UPDATE ssh_profiles SET body=?2 WHERE id=?1",
                params![profile.id.to_string(), encode(&profile)?],
            )
            .await
            .map_err(storage_error)?;
            Ok((
                Output::SshProfile(profile.clone()),
                Some(Event::SshChanged(profile)),
            ))
        }
        Command::RemoveSsh {
            profile,
            expected_revision,
        } => {
            current(db, *profile, *expected_revision).await?;
            db.execute(
                "DELETE FROM ssh_profiles WHERE id=?1",
                [profile.to_string()],
            )
            .await
            .map_err(storage_error)?;
            Ok((
                Output::SshProfiles(list(db).await?),
                Some(Event::SshRemoved { id: *profile }),
            ))
        }
        _ => Err(sailry_protocol::Fault::invalid(
            "SSH configuration command expected",
        )),
    }
}

pub(super) async fn prepare(
    database: &super::database::Database,
    command: &Command,
) -> Result<crate::ssh::Operation, Fault> {
    use crate::ssh::Task;
    let db = &database.connection;
    let (id, revision, task) = match command {
        Command::ModifySshFile {
            profile,
            expected_revision,
            path,
            action,
        } => {
            crate::ssh::files::validate_path(path)?;
            match action {
                sailry_protocol::ssh::FileAction::Copy { destination }
                | sailry_protocol::ssh::FileAction::Move { destination } => {
                    crate::ssh::files::validate_path(destination)?
                }
                _ => {}
            }
            (
                *profile,
                *expected_revision,
                Task::Files(crate::ssh::files::Operation::Modify {
                    path: path.clone(),
                    action: action.clone(),
                }),
            )
        }
        Command::BrowseSshDirectory {
            profile,
            expected_revision,
            path,
            after,
        } => {
            crate::ssh::files::validate_path(path)?;
            (
                *profile,
                *expected_revision,
                Task::Files(crate::ssh::files::Operation::List {
                    path: path.clone(),
                    after: after.clone(),
                }),
            )
        }
        Command::DownloadSshFile {
            profile,
            expected_revision,
            path,
        } => {
            crate::ssh::files::validate_path(path)?;
            (
                *profile,
                *expected_revision,
                Task::Files(crate::ssh::files::Operation::Download(path.clone())),
            )
        }
        Command::FinishSshUpload {
            profile,
            expected_revision,
            path,
            stream,
        } => {
            crate::ssh::files::validate_path(path)?;
            (
                *profile,
                *expected_revision,
                Task::Files(crate::ssh::files::Operation::Upload {
                    path: path.clone(),
                    stream: *stream,
                }),
            )
        }
        Command::InstallHost {
            profile,
            expected_revision,
            network,
        } => {
            if let Some(selection) = network {
                super::network::transport(selection).validate()?;
            }
            (*profile, *expected_revision, Task::Install(network.clone()))
        }
        Command::UpdateSshHost {
            profile,
            expected_revision,
            node,
        } => (*profile, *expected_revision, Task::Update(*node)),
        Command::CheckSsh {
            profile,
            expected_revision,
        } => (*profile, *expected_revision, Task::Check),
        Command::OpenSshTerminal {
            profile,
            expected_revision,
            launch,
        } => {
            launch.viewport.validate()?;
            (*profile, *expected_revision, Task::Terminal(launch.clone()))
        }
        Command::RunSsh {
            profile,
            expected_revision,
            command,
            timeout_ms,
        } => {
            if command.trim().is_empty()
                || *timeout_ms == 0
                || std::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(*timeout_ms))
                    .is_none()
            {
                return Err(sailry_protocol::Fault::invalid(
                    "invalid SSH command or timeout",
                ));
            }
            (
                *profile,
                *expected_revision,
                Task::Run {
                    command: command.clone(),
                    timeout_ms: *timeout_ms,
                },
            )
        }
        Command::TransferSsh {
            profile,
            expected_revision,
            transfer,
            timeout_ms,
        } => {
            crate::ssh::transfer::validate(transfer)?;
            if *timeout_ms == 0
                || std::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(*timeout_ms))
                    .is_none()
            {
                return Err(sailry_protocol::Fault::invalid(
                    "invalid SSH transfer timeout",
                ));
            }
            (
                *profile,
                *expected_revision,
                Task::Transfer {
                    root: database.worktree_root(transfer.worktree).await?,
                    profile: database.profile.clone(),
                    transfer: transfer.clone(),
                    timeout_ms: *timeout_ms,
                },
            )
        }
        _ => return Err(sailry_protocol::Fault::invalid("SSH operation expected")),
    };
    let profile = current(db, id, revision).await?;
    let credential = zeroize::Zeroizing::new(
        db.prepare("SELECT credential FROM ssh_profiles WHERE id=?1")
            .await
            .map_err(storage_error)?
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?
            .get::<String>(0)
            .map_err(storage_error)?,
    );
    let credential = serde_json::from_str(&credential).map_err(storage_error)?;
    Ok(crate::ssh::Operation {
        profile,
        credential,
        task,
    })
}
