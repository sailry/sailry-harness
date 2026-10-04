//! Role configuration shares Node transactions and durable command receipts.
use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{ErrorCode, Fault, RoleId, role};

pub(super) fn get(db: &Connection, id: RoleId) -> Result<Option<role::Profile>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM roles WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) fn list(db: &Connection) -> Result<Vec<role::Profile>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM roles ORDER BY key")
        .map_err(storage_error)?;
    query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

pub(super) fn put(
    db: &Connection,
    role: &role::Profile,
    expected: u64,
) -> Result<role::Profile, Fault> {
    role.validate()?;
    let current = get(db, role.id)?;
    let revision = current.as_ref().map_or(0, |role| role.revision);
    if revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "role revision changed",
        ));
    }
    if db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM roles WHERE key=?1 AND id<>?2)",
            params![role.key, role.id.to_string()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)?
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "role key is already in use",
        ));
    }
    if current.is_none() {
        let count: i64 = db
            .query_row("SELECT count(*) FROM roles", [], |row| row.get(0))
            .map_err(storage_error)?;
        if count >= role::MAX_PROFILES as i64 {
            return Err(Fault::new(ErrorCode::Busy, "role catalog is full"));
        }
    }
    if let Some(selection) = &role.model {
        let provider = super::agent::providers::get(db, selection.provider)?
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "role provider is unavailable"))?;
        validate_model(&provider, selection)?;
    }
    let mut role = role.clone();
    role.revision = revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "role revision exhausted"))?;
    db.execute(
        "INSERT INTO roles(id,key,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body",
        params![role.id.to_string(), role.key, encode(&role)?],
    ).map_err(storage_error)?;
    Ok(role)
}

pub(super) fn validate_model(
    provider: &sailry_protocol::conversation::Provider,
    selection: &role::Model,
) -> Result<(), Fault> {
    if !provider.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "role provider is unavailable",
        ));
    }
    let model = provider
        .models
        .iter()
        .find(|model| model.id == selection.model)
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "role model is unavailable"))?;
    if let Some(effort) = selection.effort {
        if !model.reasoning {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "role reasoning effort is unavailable",
            ));
        }
        model.validate_effort(provider.api, effort)?;
    }
    Ok(())
}

pub(super) fn remove(db: &Connection, id: RoleId, expected: u64) -> Result<(), Fault> {
    let role =
        get(db, id)?.ok_or_else(|| Fault::new(ErrorCode::NotFound, "role does not exist"))?;
    if role.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "role revision changed",
        ));
    }
    db.execute("DELETE FROM roles WHERE id=?1", [id.to_string()])
        .map_err(storage_error)?;
    Ok(())
}
