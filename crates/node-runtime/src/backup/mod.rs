//! Manual profile backups use the execution Node's storage and durable requests.
pub(crate) mod archive;
pub(crate) mod encryption;
mod git;
mod remote;
#[cfg(test)]
mod tests;

use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret, backup::Destination};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 200_000;
pub(crate) const MAX_MANIFEST: usize = 32 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Auth {
    pub identity: Option<Secret>,
    pub secret: Option<Secret>,
    pub session_token: Option<Secret>,
    #[serde(default)]
    pub passphrase: Option<Secret>,
}

#[derive(Clone)]
pub(crate) struct Plan {
    pub destination: Option<Destination>,
    pub local: Option<sailry_protocol::backup::Local>,
    pub online_enabled: bool,
    pub auth: Auth,
}

pub(crate) async fn export(
    plan: &Plan,
    packed: archive::Packed,
    id: String,
    cancel: &CancellationToken,
) -> Result<sailry_protocol::backup::Record, Fault> {
    let passphrase = encryption::passphrase(plan.auth.passphrase.as_ref())?.clone();
    let stop = cancel.clone();
    let packed = tokio::task::spawn_blocking(move || encryption::seal(packed, &passphrase, &stop))
        .await
        .map_err(io)??;
    let record = record(&packed, id)?;
    let mut exported = false;
    for location in [
        sailry_protocol::backup::Location::Local,
        sailry_protocol::backup::Location::Online,
    ] {
        let enabled = match location {
            sailry_protocol::backup::Location::Local => {
                plan.local.as_ref().is_some_and(|local| local.enabled)
            }
            sailry_protocol::backup::Location::Online => {
                plan.online_enabled && plan.destination.is_some()
            }
        };
        if enabled {
            remote::publish(plan, location, &packed, &record, cancel).await?;
            exported = true;
        }
    }
    if !exported {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "backup destination is not configured",
        ));
    }
    Ok(record)
}

pub(crate) fn record(
    packed: &archive::Packed,
    id: String,
) -> Result<sailry_protocol::backup::Record, Fault> {
    Ok(sailry_protocol::backup::Record {
        id,
        node: packed.snapshot.manifest.node,
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io)?
            .as_millis()
            .min(u64::MAX as u128) as u64,
        size_bytes: packed.size,
        files: packed.snapshot.manifest.files.len() as u32,
        sha256: packed.sha256.clone(),
    })
}

pub(crate) async fn list(
    plan: &Plan,
    cancel: &CancellationToken,
) -> Result<sailry_protocol::backup::Page, Fault> {
    use sailry_protocol::backup::{Location, Locations, Page};
    let mut records = std::collections::BTreeMap::new();
    let mut locations = std::collections::BTreeMap::<String, Locations>::new();
    let mut partial = false;
    for location in [Location::Local, Location::Online] {
        let configured = match location {
            Location::Local => plan.local.is_some(),
            Location::Online => plan.destination.is_some(),
        };
        if !configured {
            continue;
        }
        let page = remote::list(plan, location, cancel).await?;
        partial |= page.partial;
        for record in page.records {
            let entry = locations.entry(record.id.clone()).or_default();
            match location {
                Location::Local => entry.local = true,
                Location::Online => entry.online = true,
            }
            if let Some(previous) = records.get(&record.id)
                && previous != &record
            {
                return Err(Fault::invalid("backup copies have different metadata"));
            }
            records.insert(record.id.clone(), record);
        }
    }
    let mut records: Vec<_> = records.into_values().collect();
    records.sort_by(|left, right| {
        right
            .created_ms
            .cmp(&left.created_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(Page {
        records,
        partial,
        locations,
    })
}

pub(crate) use remote::restore;

pub(crate) fn cancelled(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(Fault::new(ErrorCode::Cancelled, "profile backup cancelled"))
    } else {
        Ok(())
    }
}

pub(crate) fn io(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("profile backup storage failed: {error}"),
    )
}
