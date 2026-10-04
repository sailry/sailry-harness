//! OAuth state belongs to the owning plugin configuration revisions.
use super::*;
use sailry_protocol::{CredentialId, Secret};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Grant {
    pub(super) id: CredentialId,
    pub(super) endpoint: String,
    pub(super) revision: u64,
    pub(super) credentials: Option<Secret>,
}

pub(in crate::store) fn load(
    db: &Connection,
    plugin: &str,
    server: &str,
    endpoint: &str,
    id: CredentialId,
) -> Result<(u64, Secret), Fault> {
    for (_, saved) in settings::revisions(db, plugin)? {
        if let Some(grant) = saved.authorizations.get(server)
            && grant.id == id
            && grant.endpoint == endpoint
            && let Some(credentials) = &grant.credentials
        {
            return Ok((grant.revision, credentials.clone()));
        }
    }
    Err(unavailable())
}

pub(in crate::store) fn renew(
    db: &Connection,
    plugin: &str,
    server: &str,
    endpoint: &str,
    id: CredentialId,
    revision: u64,
    credentials: Option<Secret>,
) -> Result<(), Fault> {
    if revision >= i64::MAX as u64 {
        return Err(unavailable());
    }
    let mut changed = false;
    for (saved_revision, mut saved) in settings::revisions(db, plugin)? {
        if let Some(grant) = saved.authorizations.get_mut(server)
            && grant.id == id
            && grant.endpoint == endpoint
        {
            if grant.revision != revision || grant.credentials.is_none() {
                return Err(unavailable());
            }
            grant.revision += 1;
            grant.credentials = credentials.clone();
            settings::update_revision(db, plugin, saved_revision, &saved)?;
            changed = true;
        }
    }
    if changed { Ok(()) } else { Err(unavailable()) }
}

pub(super) fn clear(db: &Connection, plugin: &str, server: &str) -> Result<(), Fault> {
    settings::edit_revisions(db, plugin, |saved| {
        if let Some(grant) = saved.authorizations.get_mut(server) {
            grant.credentials = None;
        }
    })
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "MCP authorization is unavailable or changed",
    )
}
