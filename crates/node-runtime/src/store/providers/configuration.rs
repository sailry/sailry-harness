//! Private owning provider configuration, including ordinary inline authentication fields.
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{CredentialId, ErrorCode, Fault, ProviderId, conversation::Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::authentication::Saved;
use crate::store::database::{encode, storage_error};

#[derive(Serialize, Deserialize)]
pub(in crate::store) struct Stored {
    pub(super) id: ProviderId,
    pub(super) provider: Option<Provider>,
    pub(super) listed: bool,
    pub(super) authorizations: BTreeMap<CredentialId, Saved>,
}

impl Stored {
    pub(super) fn draft(id: ProviderId) -> Self {
        Self {
            id,
            provider: None,
            listed: false,
            authorizations: BTreeMap::new(),
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
        for value in self.authorizations.values() {
            value.validate()?;
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

pub(super) fn load(db: &Connection, id: ProviderId) -> Result<Option<Stored>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM providers WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| decode(&id.to_string(), &body)).transpose()
}

pub(super) fn all(db: &Connection) -> Result<Vec<Stored>, Fault> {
    let mut query = db
        .prepare("SELECT id,body FROM providers ORDER BY rowid")
        .map_err(storage_error)?;
    query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (id, body) = row.map_err(storage_error)?;
            decode(&id, &body)
        })
        .collect()
}

pub(super) fn find(db: &Connection, id: CredentialId) -> Result<Option<Stored>, Fault> {
    let path = format!("$.authorizations.\"{id}\"");
    let value: Option<(String, Vec<u8>)> = db
        .query_row(
            "SELECT id,body FROM providers WHERE json_type(body,?1) IS NOT NULL",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    value.map(|(id, body)| decode(&id, &body)).transpose()
}

pub(super) fn save(db: &Connection, value: &Stored) -> Result<(), Fault> {
    value.validate()?;
    db.execute(
        "INSERT INTO providers(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        params![value.id.to_string(), encode(value)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn get(db: &Connection, id: ProviderId) -> Result<Option<Provider>, Fault> {
    Ok(load(db, id)?
        .filter(|stored| stored.listed)
        .and_then(|stored| stored.provider))
}

pub(in crate::store) fn list(db: &Connection) -> Result<Vec<Provider>, Fault> {
    Ok(all(db)?
        .into_iter()
        .filter(|stored| stored.listed)
        .filter_map(|stored| stored.provider)
        .collect())
}

pub(in crate::store) fn publish(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    let mut stored = load(db, provider.id)?.unwrap_or_else(|| Stored::draft(provider.id));
    stored.provider = Some(provider.clone());
    stored.listed = true;
    save(db, &stored)
}

pub(in crate::store) fn unlist(db: &Connection, id: ProviderId) -> Result<(), Fault> {
    let mut stored = load(db, id)?.ok_or_else(invalid)?;
    stored.listed = false;
    save(db, &stored)
}

pub(in crate::store) fn retain(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    let mut stored = load(db, provider.id)?.unwrap_or_else(|| Stored::draft(provider.id));
    stored.provider = Some(provider.clone());
    stored.listed = false;
    save(db, &stored)
}

pub(in crate::store) fn validate_stored(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT id,body FROM providers")
        .map_err(storage_error)?;
    let mut rows = query.query([]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        decode(
            &row.get::<_, String>(0).map_err(storage_error)?,
            &row.get::<_, Vec<u8>>(1).map_err(storage_error)?,
        )?;
    }
    Ok(())
}

fn invalid() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "invalid stored provider configuration",
    )
}
