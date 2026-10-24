//! Private owning provider configuration, including ordinary inline authentication fields.
use sailry_protocol::{CredentialId, Fault, ProviderId, conversation::Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use turso::{Connection, params};

use super::authentication::Saved;
use crate::store::database::{encode, storage_error};

#[derive(Serialize, Deserialize)]
pub(in crate::store) struct Stored {
    pub(super) id: ProviderId,
    pub(super) provider: Option<Provider>,
    pub(super) listed: bool,
    pub(super) authorizations: BTreeMap<CredentialId, Saved>,
    pub(super) quota: Option<sailry_protocol::conversation::quota::Snapshot>,
}

impl Stored {
    pub(super) fn draft(id: ProviderId) -> Self {
        Self {
            id,
            provider: None,
            listed: false,
            authorizations: BTreeMap::new(),
            quota: None,
        }
    }

    fn validate(&self) -> Result<(), Fault> {
        if self
            .provider
            .as_ref()
            .is_some_and(|provider| provider.id != self.id)
            || (self.listed && self.provider.is_none())
            || self.authorizations.len() > 256
        {
            return Err(invalid());
        }
        if let Some(provider) = &self.provider
            && provider.binding.is_some()
            && !super::presets::partner_policy(provider)?
        {
            return Err(invalid());
        }
        for value in self.authorizations.values() {
            value.validate(self.id)?;
        }
        if let Some(quota) = &self.quota {
            super::quota::validate(self.id, quota)?;
        }
        Ok(())
    }
}

fn decode(id: &str, body: &[u8]) -> Result<Stored, Fault> {
    let value: Stored = serde_json::from_slice(body).map_err(storage_error)?;
    if value.id.to_string() != id {
        return Err(invalid());
    }
    value.validate()?;
    Ok(value)
}

pub(super) async fn load(db: &Connection, id: ProviderId) -> Result<Option<Stored>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM providers WHERE id=?1")
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row([id.to_string()]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    decode(
        &id.to_string(),
        &row.get::<Vec<u8>>(0).map_err(storage_error)?,
    )
    .map(Some)
}

pub(super) async fn all(db: &Connection) -> Result<Vec<Stored>, Fault> {
    let mut rows = db
        .query("SELECT id,body FROM providers ORDER BY rowid", ())
        .await
        .map_err(storage_error)?;
    let mut stored = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        stored.push(decode(
            &row.get::<String>(0).map_err(storage_error)?,
            &row.get::<Vec<u8>>(1).map_err(storage_error)?,
        )?);
    }
    Ok(stored)
}

pub(super) async fn find(db: &Connection, id: CredentialId) -> Result<Option<Stored>, Fault> {
    let path = format!("$.authorizations.\"{id}\"");
    let mut statement = db
        .prepare("SELECT id,body FROM providers WHERE json_type(body,?1) IS NOT NULL")
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row([path]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    decode(
        &row.get::<String>(0).map_err(storage_error)?,
        &row.get::<Vec<u8>>(1).map_err(storage_error)?,
    )
    .map(Some)
}

pub(super) async fn save(db: &Connection, value: &Stored) -> Result<(), Fault> {
    value.validate()?;
    db.execute(
        "INSERT INTO providers(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        params![value.id.to_string(), encode(value)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn get(
    db: &Connection,
    id: ProviderId,
) -> Result<Option<Provider>, Fault> {
    Ok(load(db, id)
        .await?
        .filter(|stored| stored.listed)
        .and_then(|stored| stored.provider))
}

pub(in crate::store) async fn list(db: &Connection) -> Result<Vec<Provider>, Fault> {
    Ok(all(db)
        .await?
        .into_iter()
        .filter(|stored| stored.listed)
        .filter_map(|stored| stored.provider)
        .collect())
}

pub(in crate::store) async fn publish(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    let mut stored = load(db, provider.id)
        .await?
        .unwrap_or_else(|| Stored::draft(provider.id));
    stored.provider = Some(provider.clone());
    stored.listed = true;
    save(db, &stored).await
}

pub(in crate::store) async fn unlist(db: &Connection, id: ProviderId) -> Result<(), Fault> {
    let mut stored = load(db, id).await?.ok_or_else(invalid)?;
    stored.listed = false;
    save(db, &stored).await
}

pub(in crate::store) async fn retain(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    let mut stored = load(db, provider.id)
        .await?
        .unwrap_or_else(|| Stored::draft(provider.id));
    stored.provider = Some(provider.clone());
    stored.listed = false;
    save(db, &stored).await
}

pub(in crate::store) async fn validate_stored(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT id,body FROM providers")
        .await
        .map_err(storage_error)?;
    let mut rows = query.query(()).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        decode(
            &row.get::<String>(0).map_err(storage_error)?,
            &row.get::<Vec<u8>>(1).map_err(storage_error)?,
        )?;
    }
    Ok(())
}

fn invalid() -> Fault {
    Fault::invalid("invalid stored provider configuration")
}
