//! Authentication values belong to their provider configuration, never a separate store.
//! Provider binding follows sailry-code 67ae9fa0 config_credentials.rs.
use rusqlite::Connection;
use sailry_protocol::*;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::configuration::{self, Stored};
use crate::store::database::storage_error;

#[derive(Serialize, Deserialize)]
pub(super) struct Saved {
    authentication: Authentication,
    revision: u64,
    key: Option<Secret>,
    expires_at_ms: Option<u64>,
}

impl Saved {
    pub(super) fn validate(&self) -> Result<(), Fault> {
        if self.revision == 0
            || self.revision > i64::MAX as u64
            || self
                .expires_at_ms
                .is_some_and(|expiry| expiry > i64::MAX as u64)
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid stored authentication metadata",
            ));
        }
        if let Some(key) = &self.key {
            validate_key(key)?;
            if self.authentication != Authentication::ApiKey {
                crate::providers::login::Grant::decode(self.authentication, key)?;
            }
        }
        Ok(())
    }

    fn metadata(&self, provider: ProviderId, id: CredentialId) -> Credential {
        Credential {
            id,
            provider,
            authentication: self.authentication,
            revision: self.revision,
            expires_at_ms: self.expires_at_ms,
            revoked: self.key.is_none(),
        }
    }
}

pub(in crate::store) fn provider_key(
    db: &Connection,
    node: NodeId,
    provider: ProviderId,
    expected_revision: u64,
) -> Result<Option<Secret>, Fault> {
    let provider = super::current(db, provider, expected_revision)?;
    if provider.authentication != Authentication::ApiKey {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "only API keys can be read in provider settings",
        ));
    }
    let Some(reference) = provider.credential else {
        return Ok(None);
    };
    if reference.node != node {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential belongs to another Node",
        ));
    }
    resolve(db, reference.id, provider.id, Authentication::ApiKey).map(|(_, key)| Some(key))
}

pub(in crate::store) fn execute(db: &Connection, command: &Command) -> Result<Output, Fault> {
    match command {
        Command::ListCredentials => {
            let mut credentials = Vec::new();
            for stored in configuration::all(db)? {
                credentials.extend(
                    stored
                        .authorizations
                        .iter()
                        .map(|(id, value)| value.metadata(stored.id, *id)),
                );
            }
            credentials.sort_by_key(|value| value.id);
            Ok(Output::Credentials(credentials))
        }
        Command::PutCredential {
            id,
            provider,
            expected_revision,
            secret,
            expires_at_ms,
        } => put(
            db,
            Credential {
                id: *id,
                provider: *provider,
                authentication: Authentication::ApiKey,
                revision: *expected_revision,
                expires_at_ms: *expires_at_ms,
                revoked: false,
            },
            secret,
        )
        .map(Output::Credential),
        Command::RevokeCredential {
            id,
            expected_revision,
        } => {
            let mut stored = configuration::find(db, *id)?.ok_or_else(missing)?;
            let value = stored.authorizations.get_mut(id).ok_or_else(missing)?;
            value.revision = revision(value.revision, *expected_revision)?;
            value.key = None;
            let metadata = value.metadata(stored.id, *id);
            configuration::save(db, &stored)?;
            Ok(Output::Credential(metadata))
        }
        _ => Err(Fault::new(
            ErrorCode::InvalidRequest,
            "credential command expected",
        )),
    }
}

/// The expected authentication revision is incremented without changing provider selection.
pub(in crate::store) fn put(
    db: &Connection,
    mut value: Credential,
    secret: &Secret,
) -> Result<Credential, Fault> {
    validate_key(secret)?;
    let now = now_ms()?;
    if value
        .expires_at_ms
        .is_some_and(|expiry| expiry > i64::MAX as u64 || expiry <= now)
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "credential expiry must be in the future",
        ));
    }
    let previous = get(db, value.id)?;
    value.revision = revision(
        previous.as_ref().map_or(0, |value| value.revision),
        value.revision,
    )?;
    if previous.as_ref().is_some_and(|previous| {
        previous.provider != value.provider || previous.authentication != value.authentication
    }) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "credential provider and authentication cannot change",
        ));
    }
    if previous.is_none() {
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM providers,json_each(providers.body,'$.authorizations')",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if count >= 256 {
            return Err(Fault::new(ErrorCode::Busy, "credential capacity exhausted"));
        }
    }
    if value.authentication != Authentication::ApiKey {
        crate::providers::login::Grant::decode(value.authentication, secret)?;
    }
    value.revoked = false;
    let mut stored =
        configuration::load(db, value.provider)?.unwrap_or_else(|| Stored::draft(value.provider));
    stored.authorizations.insert(
        value.id,
        Saved {
            authentication: value.authentication,
            revision: value.revision,
            key: Some(secret.clone()),
            expires_at_ms: value.expires_at_ms,
        },
    );
    configuration::save(db, &stored)?;
    Ok(value)
}

pub(in crate::store) fn validate_authentication(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
    authentication: Authentication,
) -> Result<Credential, Fault> {
    let credential = validate(db, id, provider)?;
    if credential.authentication != authentication {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential authentication does not match the provider",
        ));
    }
    Ok(credential)
}

pub(in crate::store) fn validate(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
) -> Result<Credential, Fault> {
    let credential = get(db, id)?.ok_or_else(missing)?;
    validate_metadata(&credential, provider)?;
    Ok(credential)
}

pub(in crate::store) fn get(
    db: &Connection,
    id: CredentialId,
) -> Result<Option<Credential>, Fault> {
    configuration::find(db, id)?
        .map(|stored| {
            stored
                .authorizations
                .get(&id)
                .map(|value| value.metadata(stored.id, id))
                .ok_or_else(missing)
        })
        .transpose()
}

pub(in crate::store) fn resolve(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
    authentication: Authentication,
) -> Result<(Credential, Secret), Fault> {
    let stored = configuration::find(db, id)?.ok_or_else(missing)?;
    let saved = stored.authorizations.get(&id).ok_or_else(missing)?;
    let metadata = saved.metadata(stored.id, id);
    validate_metadata(&metadata, provider)?;
    if metadata.authentication != authentication {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential authentication does not match the provider",
        ));
    }
    Ok((metadata, saved.key.clone().ok_or_else(missing)?))
}

fn validate_metadata(value: &Credential, provider: ProviderId) -> Result<(), Fault> {
    if value.provider != provider || value.revoked {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential is revoked or belongs to another provider",
        ));
    }
    let now = now_ms()?;
    if value.expires_at_ms.is_some_and(|expiry| expiry <= now) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential expired; authenticate again",
        ));
    }
    Ok(())
}

fn validate_key(secret: &Secret) -> Result<(), Fault> {
    if secret.expose().is_empty() || secret.expose().len() > 65536 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "credential size must be 1 to 65536 bytes",
        ));
    }
    Ok(())
}

fn revision(actual: u64, expected: u64) -> Result<u64, Fault> {
    if actual != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "credential revision changed; reload before editing",
        ));
    }
    actual
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "credential revision exhausted"))
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "credential not found on the execution Node",
    )
}

fn now_ms() -> Result<u64, Fault> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .map_err(|_| Fault::new(ErrorCode::Internal, "system clock is invalid"))
}
