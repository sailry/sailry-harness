//! Passive observations from this Node's own SIWC model responses.
use super::{Stored, authentication, configuration};
use crate::store::{
    Ingress, Job,
    database::{Database, record, storage_error},
};
use adk_model::openai::ResponseObserver;
use reqwest::header::HeaderMap;
use sailry_protocol::{
    Authentication, Command, CredentialRef, Event, EventEnvelope, Fault, NodeId, ProviderId,
    conversation::{
        ModelApi,
        quota::{Snapshot, Window},
    },
};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, mpsc};
use turso::{Connection, transaction::TransactionBehavior};

pub(in crate::store) struct Observation {
    provider: ProviderId,
    credential: CredentialRef,
    revision: u64,
    observed_ms: u64,
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

impl Ingress {
    pub(crate) fn quota_observer(
        &self,
        provider: ProviderId,
        credential: CredentialRef,
        revision: u64,
    ) -> ResponseObserver {
        observer(self.sender.clone(), provider, credential, revision)
    }
}

fn observer(
    sender: mpsc::Sender<Job>,
    provider: ProviderId,
    credential: CredentialRef,
    revision: u64,
) -> ResponseObserver {
    Arc::new(move |headers| {
        let Some(observed_ms) = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| duration.as_millis().try_into().ok())
        else {
            return;
        };
        let (five_hour, seven_day) = parse(headers);
        let _ = sender.try_send(Job::ProviderQuota(Observation {
            provider,
            credential: credential.clone(),
            revision,
            observed_ms,
            five_hour,
            seven_day,
        }));
    })
}

fn parse(headers: &HeaderMap) -> (Option<Window>, Option<Window>) {
    let mut five_hour = None;
    let mut seven_day = None;
    let mut five_seen = false;
    let mut seven_seen = false;
    for family in ["primary", "secondary"] {
        let duration = header(headers, &format!("x-codex-{family}-window-minutes"))
            .and_then(|value| value.parse::<u64>().ok());
        let (slot, seen) = match duration {
            Some(300) => (&mut five_hour, &mut five_seen),
            Some(10080) => (&mut seven_day, &mut seven_seen),
            _ => continue,
        };
        // Two fields claiming the same window are ambiguous, not additive.
        if *seen {
            *slot = None;
            continue;
        }
        *seen = true;
        let Some(used_basis_points) = header(headers, &format!("x-codex-{family}-used-percent"))
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| (value * 100.0).round())
            .filter(|value| value.is_finite() && *value <= u32::MAX as f64)
        else {
            continue;
        };
        let reset_at_ms = header(headers, &format!("x-codex-{family}-reset-at"))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .and_then(|value| value.checked_mul(1000))
            .filter(|value| *value <= i64::MAX as u64);
        *slot = Some(Window {
            used_basis_points: used_basis_points as u32,
            reset_at_ms,
        });
    }
    (five_hour, seven_day)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let values = headers.get_all(name);
    let mut values = values.iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
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

pub(in crate::store) async fn observe(
    database: &mut Database,
    observation: Observation,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    if observation.credential.node != database.node {
        return Ok(());
    }
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
        credential: observation.credential,
        revision: observation.revision,
        observed_ms: observation.observed_ms,
        five_hour: observation.five_hour,
        seven_day: observation.seven_day,
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
    transaction.commit().await.map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
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
