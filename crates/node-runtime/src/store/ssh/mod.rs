//! Connection ownership follows sailry-code 67ae9fa0 sailry-connections.
//! Profiles and credentials now share the Node transaction and admission owner.
use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{ssh::*, *};

pub(super) mod worker;

pub(super) fn list(db: &Connection) -> Result<Vec<Profile>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM ssh_profiles ORDER BY id")
        .map_err(storage_error)?;
    query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

pub(super) fn read(db: &Connection, id: SshId) -> Result<Option<Profile>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM ssh_profiles WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

fn current(db: &Connection, id: SshId, revision: u64) -> Result<Profile, Fault> {
    let profile = read(db, id)?.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "SSH profile does not exist on this Node",
        )
    })?;
    super::commands::check_revision(profile.revision, revision)?;
    Ok(profile)
}

pub(super) fn execute(
    db: &Connection,
    command: &Command,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::ListSsh => Ok((Output::SshProfiles(list(db)?), None)),
        Command::SaveSsh {
            profile,
            expected_revision,
            credential,
        } => {
            let previous = read(db, profile.id)?;
            super::commands::check_revision(
                previous.as_ref().map_or(0, |p| p.revision),
                *expected_revision,
            )?;
            super::connections::validate(db, profile.sharing.as_ref())?;
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
                return Err(invalid("invalid SSH profile"));
            }
            if profile.host_key != previous.as_ref().and_then(|p| p.host_key.clone()) {
                return Err(invalid("host keys require explicit confirmation"));
            }
            if previous
                .as_ref()
                .is_some_and(|p| p.host != profile.host || p.port != profile.port)
            {
                profile.host_key = None;
            }
            let credential = if let Some(credential) = credential {
                if credential.authentication() != profile.authentication {
                    return Err(invalid("SSH credential authentication does not match"));
                }
                let body = zeroize::Zeroizing::new(
                    serde_json::to_string(credential).map_err(storage_error)?,
                );
                if body.len() > 65536 {
                    return Err(invalid("SSH credential exceeds the size limit"));
                }
                Some(body)
            } else {
                if previous
                    .as_ref()
                    .is_none_or(|p| p.authentication != profile.authentication)
                {
                    return Err(invalid("SSH credential is required"));
                }
                None
            };
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or_else(|| invalid("SSH profile revision exhausted"))?;
            if previous.is_none() {
                if list(db)?.len() >= 128 {
                    return Err(Fault::new(
                        ErrorCode::Busy,
                        "SSH profile capacity exhausted",
                    ));
                }
                db.execute(
                    "INSERT INTO ssh_profiles(id,body,credential) VALUES(?1,?2,?3)",
                    params![
                        profile.id.to_string(),
                        encode(&profile)?,
                        credential.as_deref().expect("new credential").as_str()
                    ],
                )
                .map_err(storage_error)?;
            } else {
                db.execute("UPDATE ssh_profiles SET body=?2,credential=COALESCE(?3,credential) WHERE id=?1",
                    params![profile.id.to_string(), encode(&profile)?, credential.as_deref().map(String::as_str)]).map_err(storage_error)?;
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
            let mut profile = current(db, *profile, *expected_revision)?;
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
                return Err(invalid("invalid SSH host key"));
            }
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or_else(|| invalid("SSH profile revision exhausted"))?;
            profile.host_key = Some(key.clone());
            db.execute(
                "UPDATE ssh_profiles SET body=?2 WHERE id=?1",
                params![profile.id.to_string(), encode(&profile)?],
            )
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
            current(db, *profile, *expected_revision)?;
            db.execute(
                "DELETE FROM ssh_profiles WHERE id=?1",
                [profile.to_string()],
            )
            .map_err(storage_error)?;
            Ok((
                Output::SshProfiles(list(db)?),
                Some(Event::SshRemoved { id: *profile }),
            ))
        }
        _ => Err(invalid("SSH configuration command expected")),
    }
}

pub(super) fn prepare(
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
        } => (*profile, *expected_revision, Task::Install),
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
                || command.len() > 65536
                || !(1..=3_600_000).contains(timeout_ms)
            {
                return Err(invalid("invalid SSH command or timeout"));
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
            if !(1..=3_600_000).contains(timeout_ms) {
                return Err(invalid("invalid SSH transfer timeout"));
            }
            (
                *profile,
                *expected_revision,
                Task::Transfer {
                    root: database.worktree_root(transfer.worktree)?,
                    profile: database.profile.clone(),
                    transfer: transfer.clone(),
                    timeout_ms: *timeout_ms,
                },
            )
        }
        _ => return Err(invalid("SSH operation expected")),
    };
    let profile = current(db, id, revision)?;
    let credential = zeroize::Zeroizing::new(
        db.query_row::<String, _, _>(
            "SELECT credential FROM ssh_profiles WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?,
    );
    let credential = serde_json::from_str(&credential).map_err(storage_error)?;
    Ok(crate::ssh::Operation {
        profile,
        credential,
        task,
    })
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
