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

pub(in crate::store) fn restore(record: &mut Record, attempt: RequestId) -> bool {
    let Some(login) = record.login.take_if(|login| login.attempt == attempt) else {
        return false;
    };
    if record.state != State::Connecting {
        return true;
    }
    if record.auth_revision != login.auth_revision {
        record.state = State::NotConnected;
        return true;
    }
    let Some(previous) = login.previous else {
        record.state = State::NotConnected;
        return true;
    };
    if record.grant_revision != login.grant_revision {
        // A same-account renewal keeps the current grant authoritative. A
        // rejection has already left Connecting and is handled above.
        record.state = if previous.auth_revision == login.auth_revision
            && (record.grant.is_some() || record.method == Kind::LocalCli)
        {
            previous.state
        } else {
            State::NotConnected
        };
    } else if record.revision == login.revision {
        let revisions = (record.revision, record.auth_revision, record.grant_revision);
        *record = *previous;
        (record.revision, record.auth_revision, record.grant_revision) = revisions;
    } else if previous.auth_revision == login.auth_revision {
        // A newer unrelated edit remains authoritative. The unchanged account
        // can leave Connecting without replacing its configuration or identity.
        record.state = previous.state;
    } else {
        record.state = State::NotConnected;
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

pub(in crate::store) fn recover(db: &Connection) -> Result<(), Fault> {
    for mut record in records(db)? {
        let changed = if let Some(login) = &record.login {
            let attempt = login.attempt;
            restore(&mut record, attempt)
        } else if record.state == State::Connecting {
            // Old interrupted attempts have no persisted account baseline.
            record.state = State::NotConnected;
            true
        } else {
            false
        };
        if changed {
            save(db, &record)?;
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
