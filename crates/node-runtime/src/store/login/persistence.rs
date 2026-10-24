use super::*;
use crate::store::database::{record, storage_error};

pub(super) async fn save(
    database: &mut Database,
    expected: &Provider,
    credential: Option<&Credential>,
    authorized: Authorized,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Fault>, Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let mut provider =
            crate::store::providers::current(&transaction, expected.id, expected.revision).await?;
        if provider != *expected {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "provider changed during authorization",
            ));
        }
        if let Some(credential) = credential {
            let current =
                crate::store::providers::authentication::get(&transaction, credential.id).await?;
            if current.as_ref() != Some(credential) {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "credential changed during authorization",
                ));
            }
        }
        authorized.grant.validate(provider.authentication)?;
        let previous = match credential.filter(|value| !value.revoked) {
            Some(credential) => Some(
                crate::store::providers::authentication::resolve(
                    &transaction,
                    credential.id,
                    provider.id,
                    provider.authentication,
                )
                .await
                .and_then(|(_, key)| Grant::decode(provider.authentication, &key))?,
            ),
            None => None,
        };
        // Only grants with comparable identities can reuse an existing authorization.
        // Opaque interactive flows receive a new identity so captured turns stay isolated.
        let same_account = previous.as_ref().is_some_and(|previous| {
            !previous.renewal_pending()
                && !matches!(previous, Grant::Plugin { .. } | Grant::ClaudeOAuth { .. })
                && previous.same_account(&authorized.grant)
        });
        let changed_account = credential.is_some() && !same_account;
        let value = Credential {
            id: credential
                .filter(|_| same_account)
                .map_or_else(CredentialId::new, |value| value.id),
            provider: provider.id,
            authentication: provider.authentication,
            account_label: None,
            revision: credential
                .filter(|_| same_account)
                .map_or(0, |value| value.revision),
            expires_at_ms: None,
            revoked: false,
        };
        let value = crate::store::providers::authentication::put(
            &transaction,
            value,
            &authorized.grant.encode()?,
        )
        .await?;
        if let Some(binding) = authorized.binding {
            crate::store::providers::authentication::bind(&transaction, &value, *binding).await?;
        }
        provider.credential = Some(CredentialRef {
            node: database.node,
            id: value.id,
        });
        if changed_account {
            provider.models.retain(|configured| {
                authorized
                    .models
                    .as_ref()
                    .is_ok_and(|models| models.iter().any(|model| model.id == configured.id))
            });
            if !provider
                .models
                .iter()
                .any(|model| model.id == provider.default_model)
            {
                provider.default_model = provider
                    .models
                    .first()
                    .map(|model| model.id.clone())
                    .unwrap_or_default();
            }
        }
        let model_error = match authorized.models {
            Ok(models) => {
                let mut completed = provider.clone();
                async {
                    super::models::seed(&transaction, &mut completed, &models, changed_account)
                        .await?;
                    crate::store::agent::providers::validate(
                        &transaction,
                        database.node,
                        &completed,
                    )
                    .await
                }
                .await
                .map(|()| provider = completed)
                .err()
            }
            Err(error) => Some(error),
        };
        let provider = crate::store::agent::providers::put(
            &transaction,
            database.node,
            &provider,
            expected.revision,
        )
        .await?;
        let mut changes = Vec::new();
        if changed_account {
            for id in crate::store::sessions::ordered(&transaction).await? {
                let mut session = crate::store::commands::session(&transaction, id).await?;
                if session.delegation.is_none()
                    && session.config.provider == provider.id
                    && session.config.credential == expected.credential
                {
                    session.config.credential = provider.credential.clone();
                    if !provider.models.is_empty()
                        && !provider
                            .models
                            .iter()
                            .any(|model| model.id == session.config.model)
                        && let Some(model) = provider
                            .models
                            .iter()
                            .find(|model| model.id == provider.default_model)
                    {
                        session.config.model = model.id.clone();
                        session.config.effort = model.default_effort;
                        session.config.features.clear();
                    } else if let Some(model) = provider
                        .models
                        .iter()
                        .find(|model| model.id == session.config.model)
                    {
                        model.retain_features(provider.api, &mut session.config.features);
                    }
                    if let Some(profile) = &mut session.profile {
                        profile.provider.credential = provider.credential.clone();
                    }
                    let (_, event) =
                        crate::store::commands::revise_session(&transaction, session).await?;
                    changes.extend(event);
                }
            }
        }
        changes.insert(0, Event::ProviderChanged(provider));
        changes
            .extend(crate::store::providers::quota::reconcile(&transaction, database.node).await?);
        let envelopes = record(&transaction, database.node, changes).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        for envelope in envelopes {
            publication.send(events, envelope);
        }
        Ok(model_error)
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
