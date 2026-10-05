use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::*;

#[cfg(test)]
mod tests;

use super::{commands, database::storage_error};
pub(super) mod assistants;
mod forks;
pub(super) use forks::{fork, origin};
mod rewind;
pub(super) use rewind::{revision as history_revision, rewind};
mod replace;
pub(super) use replace::replace;
pub(super) mod attention;
mod lifecycle;
mod start;
pub(super) use start::start;
mod order;
pub(super) use order::{ordered, reorder};
pub(super) mod location;
pub(super) mod roles;
pub(super) use lifecycle::{archive, remove, rename};

pub(super) struct Configuration {
    pub config: SessionConfig,
    pub profile: Option<SessionProfile>,
    pub roles: role::Snapshot,
}

pub(super) fn create(
    db: &Connection,
    node: NodeId,
    profile: Option<&std::path::Path>,
    project: Option<ProjectId>,
    worktree: Option<WorktreeId>,
    mut settings: Configuration,
) -> Result<(Output, Option<Event>), Fault> {
    if let Some(profile) = &mut settings.profile {
        crate::providers::login::capture(&mut profile.provider)?;
    }
    commands::validate_config(db, node, &settings.config)?;
    validate_model(db, &settings.config, settings.profile.as_ref())?;
    assistants::validate(db, project, &settings.config)?;
    let id = SessionId::new();
    let worktree = match (project, settings.config.resource) {
        (Some(project), None) => super::worktrees::select(db, project, worktree)?,
        (None, Some(resource)) => {
            let owned = super::connections::workspace(db, profile, resource)?;
            if worktree.is_some_and(|id| id != owned) {
                return Err(commands::invalid(
                    "worktree does not belong to the connection",
                ));
            }
            owned
        }
        (None, None) if settings.config.assistant.is_some() => {
            let binding = settings.config.assistant.as_ref().unwrap();
            let owned = assistants::workspace(db, profile, binding)?;
            if worktree.is_some_and(|id| id != owned) {
                return Err(commands::invalid(
                    "worktree does not belong to the plugin assistant",
                ));
            }
            owned
        }
        (None, None) => {
            if worktree.is_some() {
                return Err(commands::invalid(
                    "an unassigned conversation creates its own workspace",
                ));
            }
            super::worktrees::reserve_session(db, profile, id)?
        }
        _ => {
            return Err(commands::invalid(
                "choose a project or a connection for the conversation",
            ));
        }
    };
    let session = Session {
        id,
        archived: false,
        activity: Default::default(),
        project,
        worktree,
        revision: 1,
        config: settings.config,
        roles: settings.roles,
        profile: settings.profile,
        fork: None,
        delegation: None,
    };
    insert(db, &session)?;
    super::media::freeze(db, session.id, None)?;
    Ok((
        Output::Session(session.clone()),
        Some(Event::SessionChanged(Box::new(session))),
    ))
}

pub(super) fn validate_model(
    db: &Connection,
    config: &SessionConfig,
    profile: Option<&SessionProfile>,
) -> Result<(), Fault> {
    let provider = match profile {
        Some(profile) => Some(profile.provider.clone()),
        None => super::agent::providers::get(db, config.provider)?,
    };
    if let Some(provider) = provider {
        super::agent::providers::selection(&provider, config)?;
    }
    Ok(())
}

pub(in crate::store) fn insert(db: &Connection, session: &Session) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO sessions(id,project,worktree,revision) VALUES(?1,?2,?3,1)",
        params![
            session.id.to_string(),
            session.project.map(|id| id.to_string()),
            session.worktree.to_string()
        ],
    )
    .map_err(storage_error)?;
    commands::insert_revision(db, session)
}

pub(in crate::store) fn writable(session: &Session) -> Result<(), Fault> {
    if session.delegation.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "child conversation is read-only",
        ));
    }
    Ok(())
}

pub(super) fn profile(
    db: &Connection,
    id: SessionId,
    revision: u64,
) -> Result<Option<SessionProfile>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT profile FROM session_revisions WHERE session=?1 AND revision=?2",
            params![id.to_string(), revision as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .flatten();
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) fn import(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    profile: Option<&std::path::Path>,
    bundle: &SessionImport,
) -> Result<(Output, Option<Event>), Fault> {
    if bundle.source != caller || caller == node {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "configuration source does not match the paired caller",
        ));
    }
    if bundle.config.provider != bundle.provider.id
        || bundle.config.credential != bundle.provider.credential
        || bundle
            .config
            .credential
            .as_ref()
            .is_some_and(|reference| reference.node != caller)
        || bundle.config.credential.is_some() != bundle.secret.is_some()
        || (bundle.secret.is_none() && bundle.expires_at_ms.is_some())
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "configuration import is inconsistent",
        ));
    }
    let mut provider = bundle.provider.clone();
    if provider.authentication == Authentication::Host {
        crate::providers::login::transferable(provider.authentication)?;
    }
    // Private IDs cannot alias a provider configured independently on this Node.
    provider.id = ProviderId::new();
    provider.credential = None;
    if let Some(secret) = &bundle.secret {
        crate::providers::login::transferable(provider.authentication)?;
        let id = CredentialId::new();
        super::providers::authentication::put(
            db,
            Credential {
                id,
                provider: provider.id,
                authentication: provider.authentication,
                revision: 0,
                expires_at_ms: bundle.expires_at_ms,
                revoked: false,
            },
            secret,
        )?;
        provider.credential = Some(CredentialRef { node, id });
    }
    super::agent::providers::validate(db, node, &provider)?;
    super::providers::configuration::retain(db, &provider)?;
    let mut config = bundle.config.clone();
    config.provider = provider.id;
    config.credential = provider.credential.clone();
    let roles = roles::import(db, node, caller, &bundle.roles, &bundle.role_credentials)?;
    if let Some(binding) = &mut config.assistant {
        let installed = crate::store::plugins::get(db, &binding.package.name)?
            .filter(|info| info.summary.enabled && info.summary.digest == binding.package.digest)
            .ok_or_else(|| {
                commands::invalid("plugin assistant package is unavailable on the destination Node")
            })?;
        binding.package = installed.summary.reference();
    }
    create(
        db,
        node,
        profile,
        bundle.project,
        bundle.worktree,
        Configuration {
            config,
            profile: Some(SessionProfile {
                source: caller,
                provider,
            }),
            roles,
        },
    )
}
