use super::*;
use crate::store::database::{encode, storage_error};

pub(super) fn save(
    database: &mut Database,
    expected: &Provider,
    credential: Option<&Credential>,
    authorized: Authorized,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Fault>, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut provider =
        crate::store::providers::current(&transaction, expected.id, expected.revision)?;
    if provider != *expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider changed during authorization",
        ));
    }
    if let Some(credential) = credential {
        let current = crate::store::providers::authentication::get(&transaction, credential.id)?;
        if current.as_ref() != Some(credential) {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "credential changed during authorization",
            ));
        }
    }
    authorized.grant.validate(provider.authentication)?;
    let value = Credential {
        id: credential.map_or_else(CredentialId::new, |value| value.id),
        provider: provider.id,
        authentication: provider.authentication,
        revision: credential.map_or(0, |value| value.revision),
        expires_at_ms: None,
        revoked: false,
    };
    let value = crate::store::providers::authentication::put(
        &transaction,
        value,
        &authorized.grant.encode()?,
    )?;
    provider.credential = Some(CredentialRef {
        node: database.node,
        id: value.id,
    });
    let model_error = match authorized.models {
        Ok(models) => {
            let mut completed = provider.clone();
            super::models::seed(&transaction, &mut completed, &models)
                .and_then(|()| {
                    crate::store::agent::providers::validate(
                        &transaction,
                        database.node,
                        &completed,
                    )
                })
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
    )?;
    let event = Event::ProviderChanged(provider);
    transaction
        .execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
        .map_err(storage_error)?;
    let envelope = EventEnvelope {
        node: database.node,
        cursor: transaction.last_insert_rowid() as u64,
        event,
    };
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    Ok(model_error)
}
