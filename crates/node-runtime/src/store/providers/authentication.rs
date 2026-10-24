//! Authentication values belong to their provider configuration, never a separate store.
//! Provider binding follows sailry-code 67ae9fa0 config_credentials.rs.
use sailry_protocol::*;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use turso::Connection;

use super::configuration::{self, Stored};
use crate::store::database::storage_error;

#[derive(Serialize, Deserialize)]
pub(super) struct Saved {
    authentication: Authentication,
    revision: u64,
    key: Option<Secret>,
    expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth: Option<crate::providers::login::plugin::Target>,
}

impl Saved {
    pub(super) fn validate(&self, provider: ProviderId) -> Result<(), Fault> {
        if let Some(target) = &self.auth {
            crate::plugins::validate_name(&target.package)?;
            if target.provider != provider
                || target.flow.is_empty()
                || !matches!(
                    self.authentication,
                    Authentication::ChatGpt
                        | Authentication::ChatGptOAuth
                        | Authentication::ClaudeOAuth
                )
            {
                return Err(Fault::invalid("stored authorization binding is invalid"));
            }
        }
        if self.revision == 0
            || self.revision > i64::MAX as u64
            || self
                .expires_at_ms
                .is_some_and(|expiry| expiry > i64::MAX as u64)
        {
            return Err(Fault::invalid("invalid stored authentication metadata"));
        }
        if let Some(key) = &self.key {
            validate_key(key)?;
            if self.authentication != Authentication::ApiKey {
                crate::providers::login::Grant::decode(self.authentication, key)?;
            }
        }
        Ok(())
    }

    fn metadata(&self, provider: ProviderId, id: CredentialId) -> Result<Credential, Fault> {
        let account_label = if self.authentication == Authentication::ApiKey {
            None
        } else {
            self.key
                .as_ref()
                .map(|key| {
                    crate::providers::login::Grant::decode(self.authentication, key)
                        .map(|grant| grant.account_label().map(str::to_owned))
                })
                .transpose()?
                .flatten()
        };
        Ok(Credential {
            id,
            provider,
            authentication: self.authentication,
            account_label,
            revision: self.revision,
            expires_at_ms: self.expires_at_ms,
            revoked: self.key.is_none(),
        })
    }
}

pub(in crate::store) async fn provider_key(
    db: &Connection,
    node: NodeId,
    provider: ProviderId,
    expected_revision: u64,
) -> Result<Option<Secret>, Fault> {
    let provider = super::current(db, provider, expected_revision).await?;
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
    resolve(db, reference.id, provider.id, Authentication::ApiKey)
        .await
        .map(|(_, key)| Some(key))
}

pub(in crate::store) async fn execute(db: &Connection, command: &Command) -> Result<Output, Fault> {
    match command {
        Command::ListCredentials => {
            let mut credentials = Vec::new();
            for stored in configuration::all(db).await? {
                for (id, value) in &stored.authorizations {
                    credentials.push(value.metadata(stored.id, *id)?);
                }
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
                account_label: None,
                revision: *expected_revision,
                expires_at_ms: *expires_at_ms,
                revoked: false,
            },
            secret,
        )
        .await
        .map(Output::Credential),
        Command::RevokeCredential {
            id,
            expected_revision,
        } => {
            let mut stored = configuration::find(db, *id).await?.ok_or_else(missing)?;
            let value = stored.authorizations.get_mut(id).ok_or_else(missing)?;
            value.revision = revision(value.revision, *expected_revision)?;
            value.key = None;
            let metadata = value.metadata(stored.id, *id)?;
            configuration::save(db, &stored).await?;
            Ok(Output::Credential(metadata))
        }
        _ => Err(Fault::invalid("credential command expected")),
    }
}

/// The expected authentication revision is incremented without changing provider selection.
pub(in crate::store) async fn put(
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
        return Err(Fault::invalid("credential expiry must be in the future"));
    }
    let previous = get(db, value.id).await?;
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
            .prepare("SELECT count(*) FROM providers,json_each(providers.body,'$.authorizations')")
            .await
            .map_err(storage_error)?
            .query_row(())
            .await
            .map_err(storage_error)?
            .get(0)
            .map_err(storage_error)?;
        if count >= 256 {
            return Err(Fault::new(ErrorCode::Busy, "credential capacity exhausted"));
        }
    }
    if value.authentication != Authentication::ApiKey {
        crate::providers::login::Grant::decode(value.authentication, secret)?;
    }
    value.revoked = false;
    let mut stored = configuration::load(db, value.provider)
        .await?
        .unwrap_or_else(|| Stored::draft(value.provider));
    let saved = Saved {
        authentication: value.authentication,
        revision: value.revision,
        key: Some(secret.clone()),
        expires_at_ms: value.expires_at_ms,
        auth: stored
            .authorizations
            .get(&value.id)
            .and_then(|previous| previous.auth.clone()),
    };
    let metadata = saved.metadata(value.provider, value.id)?;
    stored.authorizations.insert(value.id, saved);
    configuration::save(db, &stored).await?;
    Ok(metadata)
}

pub(in crate::store) fn changed(node: NodeId, credential: &Credential) -> Event {
    Event::CredentialChanged {
        credential: CredentialRef {
            node,
            id: credential.id,
        },
        revision: credential.revision,
    }
}

pub(in crate::store) async fn validate_authentication(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
    authentication: Authentication,
) -> Result<Credential, Fault> {
    let credential = validate(db, id, provider).await?;
    if credential.authentication != authentication {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential authentication does not match the provider",
        ));
    }
    Ok(credential)
}

pub(in crate::store) async fn validate(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
) -> Result<Credential, Fault> {
    let credential = get(db, id).await?.ok_or_else(missing)?;
    validate_metadata(&credential, provider)?;
    Ok(credential)
}

pub(in crate::store) async fn get(
    db: &Connection,
    id: CredentialId,
) -> Result<Option<Credential>, Fault> {
    configuration::find(db, id)
        .await?
        .map(|stored| {
            stored
                .authorizations
                .get(&id)
                .map(|value| value.metadata(stored.id, id))
                .unwrap_or_else(|| Err(missing()))
        })
        .transpose()
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
    authentication: Authentication,
) -> Result<(Credential, Secret), Fault> {
    let stored = configuration::find(db, id).await?.ok_or_else(missing)?;
    let saved = stored.authorizations.get(&id).ok_or_else(missing)?;
    let metadata = saved.metadata(stored.id, id)?;
    validate_metadata(&metadata, provider)?;
    if metadata.authentication != authentication {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential authentication does not match the provider",
        ));
    }
    Ok((metadata, saved.key.clone().ok_or_else(missing)?))
}

pub(in crate::store) async fn binding(
    db: &Connection,
    id: CredentialId,
    provider: ProviderId,
) -> Result<Option<crate::providers::login::plugin::Target>, Fault> {
    let stored = configuration::find(db, id).await?.ok_or_else(missing)?;
    if stored.id != provider {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential belongs to another provider",
        ));
    }
    Ok(stored
        .authorizations
        .get(&id)
        .ok_or_else(missing)?
        .auth
        .clone())
}

pub(in crate::store) async fn bind(
    db: &Connection,
    credential: &Credential,
    target: crate::providers::login::plugin::Target,
) -> Result<(), Fault> {
    let mut stored = configuration::find(db, credential.id)
        .await?
        .ok_or_else(missing)?;
    if target.provider != credential.provider || stored.id != credential.provider {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "authorization binding belongs to another provider",
        ));
    }
    let saved = stored
        .authorizations
        .get_mut(&credential.id)
        .ok_or_else(missing)?;
    if saved.metadata(stored.id, credential.id)? != *credential {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "credential changed during authorization",
        ));
    }
    saved.auth = Some(target);
    configuration::save(db, &stored).await
}

pub(in crate::store) async fn needs_binding(db: &Connection) -> Result<bool, Fault> {
    for stored in configuration::all(db).await? {
        let Some(provider) = stored.provider else {
            continue;
        };
        for saved in stored.authorizations.values() {
            if saved.auth.is_some()
                || saved.key.is_none()
                || saved.authentication != provider.authentication
            {
                continue;
            }
            if let Some((_, package, _)) = crate::plugins::AUTHENTICATIONS
                .iter()
                .find(|(authentication, _, _)| *authentication == saved.authentication)
                && available(db, package).await?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn available(db: &Connection, package: &str) -> Result<bool, Fault> {
    if let Some(info) = crate::store::plugins::get(db, package).await? {
        return Ok(
            info.summary.enabled && info.origin == Some(sailry_protocol::plugin::Origin::Bundled)
        );
    }
    let recorded: bool = db
        .prepare("SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1)")
        .await
        .map_err(storage_error)?
        .query_row([package])
        .await
        .map_err(storage_error)?
        .get(0)
        .map_err(storage_error)?;
    Ok(!recorded)
}

/// Fill only legacy private ownership metadata; grant bytes and revisions remain unchanged.
pub(in crate::store) async fn initialize(db: &Connection) -> Result<(), Fault> {
    for mut stored in configuration::all(db).await? {
        let Some(provider) = stored.provider.as_ref() else {
            continue;
        };
        let mut changed = false;
        for saved in stored.authorizations.values_mut() {
            if saved.auth.is_some()
                || saved.key.is_none()
                || saved.authentication != provider.authentication
            {
                continue;
            }
            let Some((_, package, flow)) = crate::plugins::AUTHENTICATIONS
                .iter()
                .find(|(authentication, _, _)| *authentication == saved.authentication)
            else {
                continue;
            };
            if !available(db, package).await?
                || crate::store::plugins::get(db, package).await?.is_none()
            {
                continue;
            }
            saved.auth = Some(
                crate::store::login::model_auth::legacy_target(db, provider, package, flow).await?,
            );
            changed = true;
        }
        if changed {
            configuration::save(db, &stored).await?;
        }
    }
    Ok(())
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
        return Err(Fault::invalid("credential size must be 1 to 65536 bytes"));
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
        .ok_or_else(|| Fault::invalid("credential revision exhausted"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::conversation::{ModelApi, Provider};

    #[tokio::test]
    async fn binds_legacy_values_without_reencoding() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let handle = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let mut db = crate::store::database::connect(&handle).await.unwrap();
        db.execute_batch(include_str!("../schema.sql"))
            .await
            .unwrap();
        db.execute(
            "INSERT INTO node(singleton,identity) VALUES(1,?1)",
            [&[1u8; 32][..]],
        )
        .await
        .unwrap();
        crate::store::plugins::distribution::install(&mut db, &host)
            .await
            .unwrap();
        let provider = Provider {
            request_max_retries: None,
            stream_max_retries: None,
            id: ProviderId::new(),
            revision: 4,
            name: "Fixture".into(),
            api: ModelApi::Anthropic,
            authentication: Authentication::ClaudeOAuth,
            endpoint: "https://api.anthropic.com".into(),
            options: None,
            oauth: None,
            enabled: true,
            models: vec![],
            default_model: String::new(),
            credential: None,
            preset: None,
            binding: None,
            consent: None,
        };
        configuration::publish(&db, &provider).await.unwrap();
        // The exact private string, including whitespace, is the old storage authority.
        let key = Secret::new("{ \"kind\":\"claude_oauth\", \"access\":\"fixture-access\", \"refresh\":\"fixture-refresh\", \"expires_at_ms\":1, \"renewal_pending\":true }".into());
        let credential = put(
            &db,
            Credential {
                id: CredentialId::new(),
                provider: provider.id,
                authentication: provider.authentication,
                account_label: None,
                revision: 0,
                expires_at_ms: None,
                revoked: false,
            },
            &key,
        )
        .await
        .unwrap();
        let previous = configuration::get(&db, provider.id).await.unwrap().unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &crate::store::database::encode(
                &configuration::load(&db, provider.id)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            body["authorizations"][credential.id.to_string()]
                .get("auth")
                .is_none()
        );
        initialize(&db).await.unwrap();
        assert_eq!(
            get(&db, credential.id).await.unwrap(),
            Some(credential.clone())
        );
        let (_, retained) = resolve(&db, credential.id, provider.id, provider.authentication)
            .await
            .unwrap();
        assert_eq!(retained, key);
        assert_eq!(
            configuration::get(&db, provider.id).await.unwrap(),
            Some(previous)
        );
        let target = binding(&db, credential.id, provider.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(target.package, "claude-auth");
        assert_eq!(target.flow, "subscription");
        let before = crate::store::database::encode(
            &configuration::load(&db, provider.id)
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        initialize(&db).await.unwrap();
        assert_eq!(
            crate::store::database::encode(
                &configuration::load(&db, provider.id)
                    .await
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            before
        );
        assert!(
            binding(&db, credential.id, ProviderId::new())
                .await
                .is_err()
        );
        let mut changed = target.clone();
        changed.origin = sailry_protocol::conversation::consent::Origin::Imported {
            digest: "a".repeat(64),
        };
        assert!(
            crate::store::login::model_auth::check_binding(&db, &credential, &changed)
                .await
                .is_err()
        );
        changed = target.clone();
        changed.flow = "another".into();
        assert!(
            crate::store::login::model_auth::check_binding(&db, &credential, &changed)
                .await
                .is_err()
        );
        let mut stale = credential.clone();
        stale.revision += 1;
        assert_eq!(
            bind(&db, &stale, target).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }
}
