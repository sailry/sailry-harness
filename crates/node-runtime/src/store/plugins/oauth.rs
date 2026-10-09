//! Account grants share immutable plugin settings and explicit revocation.
use super::*;
use crate::plugins::oauth::Grant;
use sailry_protocol::{
    CredentialId,
    plugin::oauth::{Declaration, Status},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Authorized {
    pub(in crate::store) id: CredentialId,
    pub(in crate::store) revision: u64,
    pub(in crate::store) grant: Option<Grant>,
}

pub(in crate::store) fn current(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<(Info, Declaration), Fault> {
    let info = settings::current(db, package)?;
    let declaration = info
        .extension
        .as_ref()
        .and_then(|extension| extension.oauth.clone())
        .ok_or_else(|| Fault::invalid("plugin does not declare account login"))?;
    crate::plugins::oauth::validate(&declaration, info.settings.as_ref())?;
    Ok((info, declaration))
}

pub(in crate::store) fn read(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<Status, Fault> {
    let (info, declaration) = current(db, package)?;
    let stored = settings::stored(db, &info)?;
    Ok(Status {
        package: package.clone(),
        configured: stored
            .oauth
            .as_ref()
            .and_then(|saved| saved.grant.as_ref())
            .is_some_and(|grant| grant.declaration == declaration),
    })
}

pub(in crate::store) fn save(
    db: &Connection,
    package: &plugin::Reference,
    grant: Grant,
) -> Result<Info, Fault> {
    let (mut info, declaration) = current(db, package)?;
    if grant.declaration != declaration {
        return Err(Fault::invalid("plugin account declaration changed"));
    }
    let mut stored = settings::stored(db, &info)?;
    clear(db, &package.name)?;
    for name in &declaration.credentials {
        stored
            .slots
            .insert(name.clone(), Some(grant.access_token.clone()));
    }
    stored.oauth = Some(Authorized {
        id: CredentialId::new(),
        revision: 1,
        grant: Some(grant),
    });
    settings::persist(db, &mut info, &stored)?;
    Ok(info)
}

pub(in crate::store) fn revoke(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<Info, Fault> {
    let (mut info, declaration) = current(db, package)?;
    clear(db, &package.name)?;
    let mut stored = settings::stored(db, &info)?;
    for name in &declaration.credentials {
        stored.slots.insert(name.clone(), None);
    }
    stored.oauth = None;
    settings::persist(db, &mut info, &stored)?;
    Ok(info)
}

pub(super) fn clear(db: &Connection, plugin: &str) -> Result<(), Fault> {
    settings::edit_revisions(db, plugin, |saved| {
        if let Some(authorization) = &mut saved.oauth {
            if let Some(grant) = &authorization.grant {
                for name in &grant.declaration.credentials {
                    saved.slots.insert(name.clone(), None);
                }
            }
            authorization.grant = None;
        }
    })
}

pub(in crate::store) fn load(
    db: &Connection,
    plugin: &str,
    id: CredentialId,
) -> Result<(u64, Grant), Fault> {
    // Removal or disabling prevents new outbound account requests, including old captures.
    if !required(db, plugin)?.summary.enabled {
        return Err(unavailable());
    }
    for (_, saved) in settings::revisions(db, plugin)? {
        if let Some(authorization) = saved.oauth
            && authorization.id == id
            && let Some(grant) = authorization.grant
        {
            return Ok((authorization.revision, grant));
        }
    }
    Err(unavailable())
}

pub(in crate::store) fn renew(
    db: &Connection,
    plugin: &str,
    id: CredentialId,
    revision: u64,
    grant: Grant,
) -> Result<(), Fault> {
    let (current, previous) = load(db, plugin, id)?;
    if current != revision
        || previous.declaration != grant.declaration
        || previous.configuration != grant.configuration
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin account authorization changed",
        ));
    }
    let next = revision
        .checked_add(1)
        .filter(|next| *next <= i64::MAX as u64)
        .ok_or_else(unavailable)?;
    for (saved_revision, mut saved) in settings::revisions(db, plugin)? {
        if let Some(authorization) = &mut saved.oauth
            && authorization.id == id
        {
            if authorization.revision != revision || authorization.grant.is_none() {
                return Err(unavailable());
            }
            authorization.revision = next;
            authorization.grant = Some(grant.clone());
            for name in &grant.declaration.credentials {
                saved
                    .slots
                    .insert(name.clone(), Some(grant.access_token.clone()));
            }
            settings::update_revision(db, plugin, saved_revision, &saved)?;
        }
    }
    Ok(())
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "plugin account authorization is unavailable or changed",
    )
}

#[cfg(test)]
mod tests;
