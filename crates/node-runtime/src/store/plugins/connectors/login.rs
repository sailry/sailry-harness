//! Private login recovery retains the original account without reversing fences.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Pending {
    pub attempt: RequestId,
    pub revision: u64,
    pub auth_revision: u64,
    pub grant_revision: u64,
    pub previous: Option<Box<Record>>,
}

pub(in crate::store) fn begin(record: &mut Record, attempt: RequestId, previous: Option<Record>) {
    record.login = Some(Pending {
        attempt,
        revision: record.revision,
        auth_revision: record.auth_revision,
        grant_revision: record.grant_revision,
        previous: previous.map(|mut previous| {
            previous.login = None;
            Box::new(previous)
        }),
    });
}

pub(in crate::store) enum Outcome {
    Connected(Box<crate::plugins::connectors::Target>),
    Cancelled,
    Failed,
}

/// Finalizes only the owned attempt, retaining the current credential fences.
pub(in crate::store) fn finish(
    record: &mut Record,
    attempt: RequestId,
    outcome: Outcome,
) -> Result<bool, Fault> {
    if record.login.as_ref().map(|login| login.attempt) != Some(attempt) {
        return Ok(false);
    }
    if let Outcome::Connected(target) = outcome {
        let revision = record
            .grant_revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| Fault::new(ErrorCode::Conflict, "connector grant revision exhausted"))?;
        if record
            .login
            .as_ref()
            .is_some_and(|login| login.revision > 1)
        {
            record.auth_revision = record.revision;
        }
        record.identity = target.identity;
        record.fields = target.fields;
        record.secrets = target.secrets;
        record.grant_revision = revision;
        record.grant = target.grant.map(|grant| Grant {
            access: grant.access,
            refresh: grant.refresh,
            expires_at_ms: grant.expires_at_ms,
            extra: grant.extra,
        });
        record.state = State::Connected;
        record.checked_at_ms = Some(crate::plugins::connectors::now());
        record.login = None;
        return Ok(true);
    }
    let first = record
        .login
        .as_ref()
        .is_some_and(|login| login.previous.is_none());
    if !restore(record, attempt) {
        return Ok(false);
    }
    if matches!(outcome, Outcome::Failed) && first {
        record.state = State::Failed;
    }
    Ok(true)
}

fn restore(record: &mut Record, attempt: RequestId) -> bool {
    let Some(login) = record.login.take_if(|login| login.attempt == attempt) else {
        return false;
    };
    let renewed = record.grant_revision != login.grant_revision;
    if record.state != State::Connecting && !renewed {
        return true;
    }
    let Some(previous) = login
        .previous
        .filter(|_| record.auth_revision == login.auth_revision)
    else {
        if record.state == State::Connecting {
            record.state = State::NotConnected;
        }
        return true;
    };
    // A same-account renewal keeps the current grant authoritative.
    let account = previous.auth_revision == login.auth_revision
        && (record.grant.is_some() || record.method == Kind::LocalCli);
    if record.revision == login.revision {
        // The attempt's unconfirmed edits are discarded; fences never move back.
        let current = std::mem::replace(record, *previous);
        (record.revision, record.auth_revision, record.grant_revision) = (
            current.revision,
            current.auth_revision,
            current.grant_revision,
        );
        if renewed {
            record.grant = current.grant;
            record.state = if current.state != State::Connecting {
                current.state
            } else if account {
                record.state
            } else {
                State::NotConnected
            };
        }
    } else if record.state == State::Connecting {
        // A newer unrelated edit remains authoritative. The unchanged account
        // can leave Connecting without replacing its configuration or identity.
        record.state = if previous.auth_revision == login.auth_revision && (!renewed || account) {
            previous.state
        } else {
            State::NotConnected
        };
    }
    true
}

pub(super) fn usable(record: &Record) -> bool {
    record.state == State::Connecting
        && record.login.as_ref().is_some_and(|login| {
            login.auth_revision == record.auth_revision
                && login.previous.as_ref().is_some_and(|previous| {
                    previous.auth_revision == record.auth_revision
                        && matches!(previous.state, State::Connected | State::Failed)
                })
        })
}

pub(in crate::store) async fn recover(db: &Connection) -> Result<(), Fault> {
    for mut record in records(db).await? {
        let changed = if let Some(login) = &record.login {
            let attempt = login.attempt;
            finish(&mut record, attempt, Outcome::Cancelled)?
        } else if record.state == State::Connecting {
            // Old interrupted attempts have no persisted account baseline.
            record.state = State::NotConnected;
            true
        } else {
            false
        };
        if changed {
            save(db, &record).await?;
        }
    }
    Ok(())
}

pub(super) fn valid(record: &Record) -> bool {
    record.login.as_ref().is_none_or(|login| {
        login.revision != 0
            && login.revision <= record.revision
            && login.auth_revision != 0
            && login.auth_revision <= login.revision
            && login.auth_revision <= record.auth_revision
            && login.grant_revision != 0
            && login.grant_revision <= record.grant_revision
            && login
                .previous
                .as_ref()
                .map_or(login.revision == 1, |previous| {
                    previous.login.is_none()
                        && previous.id == record.id
                        && previous.plugin == record.plugin
                        && previous.connector == record.connector
                        && previous.revision.checked_add(1) == Some(login.revision)
                        && previous.auth_revision <= login.auth_revision
                        && previous.grant_revision <= login.grant_revision
                        && login.grant_revision - previous.grant_revision <= 1
                        && previous.state != State::Connecting
                        && validate(previous).is_ok()
                })
    })
}
