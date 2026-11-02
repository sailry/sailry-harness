//! A roster belongs to a session revision, never to the currently connected controller.
use super::*;
use crate::store::{agent::providers, database::Database};

/// Replace the roster while retaining the team, lead instructions and remaining overrides.
pub(in crate::store) async fn select(
    db: &Connection,
    node: NodeId,
    references: &[role::Reference],
    previous: role::Snapshot,
) -> Result<role::Snapshot, Fault> {
    let ids: std::collections::BTreeSet<_> = references.iter().map(|role| role.id).collect();
    if ids.len() != references.len() {
        return Err(Fault::invalid("session roles must be distinct"));
    }
    let mut profiles = Vec::with_capacity(references.len());
    for reference in references {
        let profile = crate::store::roles::get(db, reference.id)
            .await?
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "role does not exist"))?;
        crate::store::commands::check_revision(profile.revision, reference.revision)?;
        profiles.push(profile);
    }
    let mut models = previous.models;
    models.retain(|role, _| ids.contains(role));
    complete(
        db,
        node,
        role::Snapshot {
            execution: conversation::Execution::Team,
            profiles,
            providers: vec![],
            team: previous.team,
            instructions: previous.instructions,
            models,
        },
    )
    .await
}

pub(in crate::store) async fn capture(
    db: &Connection,
    node: NodeId,
    profiles: Vec<role::Profile>,
) -> Result<role::Snapshot, Fault> {
    complete(
        db,
        node,
        role::Snapshot {
            profiles,
            ..Default::default()
        },
    )
    .await
}

/// Resolve execution at admission and freeze the selected roster.
pub(in crate::store) async fn execution(
    db: &Connection,
    node: NodeId,
    execution: conversation::Execution,
    reference: Option<&team::Selection>,
    models: std::collections::BTreeMap<RoleId, role::Model>,
) -> Result<role::Snapshot, Fault> {
    if execution == conversation::Execution::Model {
        if reference.is_some() || !models.is_empty() {
            return Err(Fault::invalid(
                "model execution cannot select a team or member models",
            ));
        }
        return Ok(role::Snapshot {
            execution,
            ..Default::default()
        });
    }
    let mut snapshot = role::Snapshot {
        execution,
        models,
        ..Default::default()
    };
    match reference {
        None => snapshot.profiles = crate::store::roles::list(db).await?,
        Some(reference) => {
            let team = crate::store::teams::get(db, reference.id)
                .await?
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "team does not exist"))?;
            if let Some(revision) = reference.revision {
                crate::store::commands::check_revision(team.revision, revision)?;
            }
            for member in &team.members {
                snapshot.profiles.push(
                    crate::store::roles::get(db, *member)
                        .await?
                        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "role does not exist"))?,
                );
            }
            snapshot.team = Some(team.reference());
            snapshot.instructions = team.instructions;
        }
    }
    complete(db, node, snapshot).await
}

/// Set or clear one member override for future delegation.
pub(in crate::store) async fn assign(
    db: &Connection,
    node: NodeId,
    mut snapshot: role::Snapshot,
    role: RoleId,
    model: Option<role::Model>,
) -> Result<role::Snapshot, Fault> {
    if !snapshot.profiles.iter().any(|profile| profile.id == role) {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "role is not in the session roster",
        ));
    }
    match model {
        Some(model) => snapshot.models.insert(role, model),
        None => snapshot.models.remove(&role),
    };
    complete(db, node, snapshot).await
}

/// Capture current providers for every role model and member override.
async fn complete(
    db: &Connection,
    node: NodeId,
    mut snapshot: role::Snapshot,
) -> Result<role::Snapshot, Fault> {
    snapshot.providers.clear();
    let required: std::collections::BTreeSet<_> = snapshot
        .profiles
        .iter()
        .filter_map(|profile| profile.model.as_ref())
        .chain(snapshot.models.values())
        .map(|model| model.provider)
        .collect();
    for id in required {
        let mut provider = providers::get(db, id)
            .await?
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "role provider is unavailable"))?;
        crate::store::providers::presets::capture(db, &mut provider).await?;
        crate::providers::login::capture(&mut provider)?;
        snapshot.providers.push(provider);
    }
    validate(db, node, &snapshot).await?;
    Ok(snapshot)
}

async fn validate(db: &Connection, node: NodeId, snapshot: &role::Snapshot) -> Result<(), Fault> {
    snapshot.validate()?;
    for provider in &snapshot.providers {
        providers::validate(db, node, provider).await?;
    }
    for model in snapshot
        .profiles
        .iter()
        .filter_map(|profile| profile.model.as_ref())
        .chain(snapshot.models.values())
    {
        let provider = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == model.provider)
            .expect("snapshot provider references were validated");
        crate::store::roles::validate_model(provider, model)?;
    }
    Ok(())
}

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    revision: u64,
) -> Result<role::Snapshot, Fault> {
    let body: Vec<u8> = db
        .prepare("SELECT roles FROM session_revisions WHERE session=?1 AND revision=?2")
        .await
        .map_err(storage_error)?
        .query_row(params![session.to_string(), revision as i64])
        .await
        .and_then(|row| row.get(0))
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(in crate::store) async fn export(
    database: &Database,
    snapshot: &role::Snapshot,
) -> Result<Vec<role::ProviderSecret>, Fault> {
    let mut secrets = Vec::new();
    for provider in &snapshot.providers {
        let Some(reference) = &provider.credential else {
            continue;
        };
        crate::providers::login::transferable(provider.authentication)?;
        let secret = database
            .resolve_credential(reference, provider.id, provider.authentication)
            .await?;
        let metadata =
            crate::store::providers::authentication::get(&database.connection, reference.id)
                .await?
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "source credential does not exist")
                })?;
        secrets.push(role::ProviderSecret {
            provider: provider.id,
            secret,
            expires_at_ms: metadata.expires_at_ms,
        });
    }
    Ok(secrets)
}

pub(in crate::store) async fn import(
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
        return Err(Fault::invalid("role credential import is inconsistent"));
    }
    let mut snapshot = snapshot.clone();
    for provider in &mut snapshot.providers {
        crate::store::providers::presets::capture(db, provider).await?;
        crate::providers::login::capture(provider)?;
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
                    account_label: None,
                    revision: 0,
                    expires_at_ms: credential.expires_at_ms,
                    revoked: false,
                },
                &credential.secret,
            )
            .await?;
            provider.credential = Some(CredentialRef { node, id });
        }
        for model in snapshot
            .profiles
            .iter_mut()
            .filter_map(|profile| profile.model.as_mut())
            .chain(snapshot.models.values_mut())
        {
            if model.provider == original {
                model.provider = provider.id;
            }
        }
        let mut configured = provider.clone();
        if !crate::store::providers::presets::partner_policy(&configured)? {
            configured.binding = None;
        }
        crate::store::providers::configuration::retain(db, &configured).await?;
    }
    validate(db, node, &snapshot).await?;
    Ok(snapshot)
}

/// Resolve declarations for a newly admitted root turn without changing its selected roles.
pub(in crate::store) async fn refresh(
    db: &Connection,
    snapshot: &mut role::Snapshot,
) -> Result<(), Fault> {
    for provider in &mut snapshot.providers {
        crate::store::providers::presets::refresh(db, provider).await?;
        crate::providers::login::capture(provider)?;
    }
    Ok(())
}

pub(in crate::store) async fn for_turn(
    db: &Connection,
    turn: TurnId,
) -> Result<role::Snapshot, Fault> {
    let body:Vec<u8>=db.prepare("SELECT coalesce(t.roles,r.roles) FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1").await.map_err(storage_error)?.query_row([turn.to_string()]).await.and_then(|row|row.get(0)).map_err(storage_error)?;
    let snapshot: role::Snapshot = serde_json::from_slice(&body).map_err(storage_error)?;
    snapshot
        .validate()
        .map_err(|_| storage_error("invalid captured roles"))?;
    Ok(snapshot)
}
