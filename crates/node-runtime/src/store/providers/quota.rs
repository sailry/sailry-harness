//! Stored account snapshots retain their original credential and revision owner.
use super::{Stored, authentication, configuration};
#[cfg(test)]
use crate::store::database::{Database, record, storage_error};
use sailry_protocol::{
    Authentication, Command, Event, Fault, NodeId, ProviderId,
    conversation::{ModelApi, quota::Snapshot},
};
#[cfg(test)]
use tokio::sync::broadcast;
use turso::Connection;
#[cfg(test)]
use turso::transaction::TransactionBehavior;

#[cfg(test)]
use sailry_protocol::{CredentialRef, EventEnvelope, conversation::quota::Window};

#[cfg(test)]
struct Observation {
    provider: ProviderId,
    credential: CredentialRef,
    revision: u64,
    observed_ms: u64,
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

pub(in crate::store) fn validate(provider: ProviderId, quota: &Snapshot) -> Result<(), Fault> {
    if quota.provider != provider
        || quota.revision == 0
        || quota.revision > i64::MAX as u64
        || quota.observed_ms > i64::MAX as u64
        || [&quota.five_hour, &quota.seven_day]
            .into_iter()
            .flatten()
            .any(|window| {
                window
                    .reset_at_ms
                    .is_some_and(|reset| reset == 0 || reset > i64::MAX as u64)
            })
    {
        return Err(Fault::invalid("invalid provider quota observation"));
    }
    Ok(())
}

async fn matches(
    db: &Connection,
    node: NodeId,
    stored: &Stored,
    quota: &Snapshot,
) -> Result<bool, Fault> {
    let Some(provider) = &stored.provider else {
        return Ok(false);
    };
    if !stored.listed
        || !provider.enabled
        || provider.authentication != Authentication::ChatGpt
        || provider.api != ModelApi::Responses
        || provider.endpoint.trim_end_matches('/') != crate::providers::login::CHATGPT_ENDPOINT
        || quota.credential.node != node
        || provider.credential.as_ref() != Some(&quota.credential)
    {
        return Ok(false);
    }
    Ok(authentication::get(db, quota.credential.id)
        .await?
        .is_some_and(|credential| {
            credential.provider == provider.id
                && credential.authentication == Authentication::ChatGpt
                && !credential.revoked
                && credential.revision == quota.revision
        }))
}

#[cfg(test)]
async fn observe(
    database: &mut Database,
    observation: Observation,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let write = database.write().await?;
    for attempt in 0..=crate::store::database::conflict::RETRIES {
        let result = observe_once(database, &observation, events).await;
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        if !crate::store::database::conflict::again(&result, attempt).await {
            return result;
        }
    }
    unreachable!("the final attempt returns its result")
}

#[cfg(test)]
async fn observe_once(
    database: &mut Database,
    observation: &Observation,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    if observation.credential.node != database.node {
        return Ok(());
    }
    let publication = database.publication.clone();
    let node = database.node;
    let transaction = database
        .connection
        .transaction_with_behavior(TransactionBehavior::Concurrent)
        .await
        .map_err(storage_error)?;
    let Some(mut stored) = configuration::load(&transaction, observation.provider).await? else {
        return Ok(());
    };
    let mut snapshot = Snapshot {
        provider: observation.provider,
        credential: observation.credential.clone(),
        revision: observation.revision,
        observed_ms: observation.observed_ms,
        five_hour: observation.five_hour.clone(),
        seven_day: observation.seven_day.clone(),
    };
    if !matches(&transaction, node, &stored, &snapshot).await? {
        return Ok(());
    }
    if let Some(previous) = &stored.quota
        && previous.credential == snapshot.credential
        && previous.revision == snapshot.revision
    {
        if previous.observed_ms > snapshot.observed_ms {
            return Ok(());
        }
        if previous.observed_ms == snapshot.observed_ms {
            // Millisecond ties cannot establish source order. Unknown wins;
            // another known value cannot replace the existing observation.
            snapshot.five_hour = snapshot.five_hour.and(previous.five_hour.clone());
            snapshot.seven_day = snapshot.seven_day.and(previous.seven_day.clone());
        }
    }
    if stored.quota.as_ref() == Some(&snapshot) {
        return Ok(());
    }
    let previous = stored
        .quota
        .as_ref()
        .filter(|quota| present(quota))
        .cloned();
    let quota = present(&snapshot).then_some(snapshot.clone());
    // Keep the latest unknown observation too, so an older response cannot restore it.
    stored.quota = Some(snapshot);
    configuration::save(&transaction, &stored).await?;
    let changes = if previous == quota {
        Vec::new()
    } else {
        vec![Event::ProviderQuotaChanged {
            provider: stored.id,
            quota,
        }]
    };
    let envelopes = record(&transaction, node, changes).await?;
    let publication = publication.lock().await;
    crate::store::database::conflict::commit(transaction).await?;
    for envelope in envelopes {
        publication.send(events, envelope);
    }
    Ok(())
}

pub(in crate::store) async fn list(db: &Connection, node: NodeId) -> Result<Vec<Snapshot>, Fault> {
    let mut quotas = Vec::new();
    for stored in configuration::all(db).await? {
        if let Some(quota) = &stored.quota
            && present(quota)
            && matches(db, node, &stored, quota).await?
        {
            quotas.push(quota.clone());
        }
    }
    Ok(quotas)
}

pub(in crate::store) fn affected(command: &Command) -> bool {
    matches!(
        command,
        Command::PutProvider { .. }
            | Command::SaveProvider { .. }
            | Command::RemoveProvider { .. }
            | Command::PutCredential { .. }
            | Command::RevokeCredential { .. }
    )
}

pub(in crate::store) async fn reconcile(
    db: &Connection,
    node: NodeId,
) -> Result<Vec<Event>, Fault> {
    let mut events = Vec::new();
    for mut stored in configuration::all(db).await? {
        if let Some(quota) = &stored.quota
            && !matches(db, node, &stored, quota).await?
        {
            let visible = present(quota);
            stored.quota = None;
            configuration::save(db, &stored).await?;
            if visible {
                events.push(Event::ProviderQuotaChanged {
                    provider: stored.id,
                    quota: None,
                });
            }
        }
    }
    Ok(events)
}

fn present(quota: &Snapshot) -> bool {
    quota.five_hour.is_some() || quota.seven_day.is_some()
}

#[cfg(test)]
mod tests;
