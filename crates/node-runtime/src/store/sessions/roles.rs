//! A roster belongs to a session revision, never to the currently connected controller.
use super::*;
use crate::store::{agent::providers, database::Database};

pub(in crate::store) fn select(
    db: &Connection,
    node: NodeId,
    references: &[role::Reference],
) -> Result<role::Snapshot, Fault> {
    let ids: std::collections::BTreeSet<_> = references.iter().map(|role| role.id).collect();
    if references.len() > role::MAX_PROFILES || ids.len() != references.len() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "session roles must be distinct and bounded",
        ));
    }
    let profiles = references
        .iter()
        .map(|reference| {
            let profile = crate::store::roles::get(db, reference.id)?
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "role does not exist"))?;
            crate::store::commands::check_revision(profile.revision, reference.revision)?;
            Ok(profile)
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    capture(db, node, profiles)
}

pub(in crate::store) fn capture(
    db: &Connection,
    node: NodeId,
    profiles: Vec<role::Profile>,
) -> Result<role::Snapshot, Fault> {
    let mut snapshot = role::Snapshot {
        profiles,
        providers: vec![],
    };
    for profile in &snapshot.profiles {
        if let Some(model) = &profile.model
            && !snapshot
                .providers
                .iter()
                .any(|provider| provider.id == model.provider)
        {
            let provider = providers::get(db, model.provider)?.ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "role provider is unavailable")
            })?;
            snapshot.providers.push(provider);
        }
    }
    validate(db, node, &snapshot)?;
    Ok(snapshot)
}

fn validate(db: &Connection, node: NodeId, snapshot: &role::Snapshot) -> Result<(), Fault> {
    snapshot.validate()?;
    for provider in &snapshot.providers {
        providers::validate(db, node, provider)?;
    }
    for profile in &snapshot.profiles {
        if let Some(model) = &profile.model {
            let provider = snapshot
                .providers
                .iter()
                .find(|provider| provider.id == model.provider)
                .expect("snapshot provider references were validated");
            crate::store::roles::validate_model(provider, model)?;
        }
    }
    Ok(())
}

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
    revision: u64,
) -> Result<role::Snapshot, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT roles FROM session_revisions WHERE session=?1 AND revision=?2",
            params![session.to_string(), revision as i64],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(in crate::store) fn export(
    database: &Database,
    snapshot: &role::Snapshot,
) -> Result<Vec<role::ProviderSecret>, Fault> {
    for provider in &snapshot.providers {
        if provider.authentication == Authentication::Host {
            crate::providers::login::transferable(provider.authentication)?;
        }
    }
    snapshot
        .providers
        .iter()
        .filter_map(|provider| {
            provider
                .credential
                .as_ref()
                .map(|reference| (provider, reference))
        })
        .map(|(provider, reference)| {
            crate::providers::login::transferable(provider.authentication)?;
            let secret =
                database.resolve_credential(reference, provider.id, provider.authentication)?;
            let metadata =
                crate::store::providers::authentication::get(&database.connection, reference.id)?
                    .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "source credential does not exist")
                })?;
            Ok(role::ProviderSecret {
                provider: provider.id,
                secret,
                expires_at_ms: metadata.expires_at_ms,
            })
        })
        .collect()
}

pub(in crate::store) fn import(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    snapshot: &role::Snapshot,
    credentials: &[role::ProviderSecret],
) -> Result<role::Snapshot, Fault> {
    use std::collections::BTreeSet;
    snapshot.validate()?;
    let required: BTreeSet<_> = snapshot
        .providers
        .iter()
        .filter(|provider| provider.credential.is_some())
        .map(|provider| provider.id)
        .collect();
    let supplied: BTreeSet<_> = credentials
        .iter()
        .map(|credential| credential.provider)
        .collect();
    if required != supplied
        || supplied.len() != credentials.len()
        || snapshot.providers.iter().any(|provider| {
            provider
                .credential
                .as_ref()
                .is_some_and(|reference| reference.node != caller)
        })
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "role credential import is inconsistent",
        ));
    }
    let mut snapshot = snapshot.clone();
    for provider in &mut snapshot.providers {
        if provider.authentication == Authentication::Host {
            crate::providers::login::transferable(provider.authentication)?;
        }
        let original = provider.id;
        provider.id = ProviderId::new();
        provider.credential = None;
        if let Some(credential) = credentials
            .iter()
            .find(|credential| credential.provider == original)
        {
            let id = CredentialId::new();
            crate::providers::login::transferable(provider.authentication)?;
            crate::store::providers::authentication::put(
                db,
                Credential {
                    id,
                    provider: provider.id,
                    authentication: provider.authentication,
                    revision: 0,
                    expires_at_ms: credential.expires_at_ms,
                    revoked: false,
                },
                &credential.secret,
            )?;
            provider.credential = Some(CredentialRef { node, id });
        }
        for profile in &mut snapshot.profiles {
            if let Some(model) = &mut profile.model
                && model.provider == original
            {
                model.provider = provider.id;
            }
        }
        crate::store::providers::configuration::retain(db, provider)?;
    }
    validate(db, node, &snapshot)?;
    Ok(snapshot)
}
