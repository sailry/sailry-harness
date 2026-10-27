//! Role configuration shares Node transactions and durable command receipts.
use super::database::{encode, storage_error};
use sailry_protocol::{ErrorCode, Fault, RoleId, role};
use turso::{Connection, params};
pub(super) mod approvals;

const EXPLORE_INSTRUCTIONS: &str = "Explore the assigned scope using read-only tools. Read files the task names or needs, distinguish confirmed facts from inferences, and return concise findings with exact paths and unresolved questions. Do not modify resources or request user input.";
const PREVIOUS_EXPLORE: &str = "Explore the assigned scope using read-only tools. Read existing code and relevant project instructions, distinguish confirmed facts from inferences, and return concise findings with exact paths and unresolved questions. Do not modify resources or request user input.";

fn explore(id: RoleId) -> role::Profile {
    role::Profile {
        id,
        revision: 0,
        key: "explore".into(),
        name: "Explore".into(),
        appearance: None,
        description: "Read existing code and report findings with exact paths".into(),
        read_only: true,
        model: None,
        max_turns: None,
        skills: Vec::new(),
        tools: None,
        instructions: EXPLORE_INSTRUCTIONS.into(),
    }
}

pub(super) async fn initialize(db: &Connection) -> Result<(), Fault> {
    put(db, &explore(RoleId::new()), 0).await?;
    put(
        db,
        &role::Profile {
            id: RoleId::new(),
            revision: 0,
            key: "research".into(),
            name: "Research".into(),
            appearance: None,
            description: "Investigate public sources and return supported conclusions".into(),
            read_only: true,
            model: None,
            max_turns: Some(8),
            skills: vec!["web-search:research".into()],
            tools: None,
            instructions: "Research the assigned question using the selected web-search:research skill and available read-only tools. Return supported conclusions with source links, uncertainty and conflicting evidence to the parent. Keep the inherited permissions and the task's budget; do not modify resources or ask the user for input.".into(),
        },
        0,
    ).await?;
    Ok(())
}

async fn outdated(db: &Connection) -> Result<Option<role::Profile>, Fault> {
    let row = db
        .prepare("SELECT body FROM roles WHERE key='explore'")
        .await
        .map_err(storage_error)?
        .query_row(())
        .await;
    let row = match row {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let profile: role::Profile =
        serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error)?;
    let mut previous = explore(profile.id);
    previous.revision = 1;
    previous.instructions = PREVIOUS_EXPLORE.into();
    Ok((profile == previous).then_some(profile))
}

pub(in crate::store) async fn needs_refresh(db: &Connection) -> Result<bool, Fault> {
    Ok(outdated(db).await?.is_some())
}

/// Called only by the existing backed-up, current-format update transaction.
pub(in crate::store) async fn refresh(db: &Connection) -> Result<(), Fault> {
    if let Some(mut profile) = outdated(db).await? {
        profile.instructions = EXPLORE_INSTRUCTIONS.into();
        put(db, &profile, profile.revision).await?;
    }
    Ok(())
}

pub(super) async fn get(db: &Connection, id: RoleId) -> Result<Option<role::Profile>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM roles WHERE id=?1")
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row([id.to_string()]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
        .map(Some)
        .map_err(storage_error)
}

pub(super) async fn list(db: &Connection) -> Result<Vec<role::Profile>, Fault> {
    let mut rows = db
        .query("SELECT body FROM roles ORDER BY key", ())
        .await
        .map_err(storage_error)?;
    let mut profiles = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        profiles.push(
            serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?,
        );
    }
    Ok(profiles)
}

pub(super) async fn put(
    db: &Connection,
    role: &role::Profile,
    expected: u64,
) -> Result<role::Profile, Fault> {
    role.validate()?;
    let current = get(db, role.id).await?;
    let revision = current.as_ref().map_or(0, |role| role.revision);
    if revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "role revision changed",
        ));
    }
    if db
        .prepare("SELECT EXISTS(SELECT 1 FROM roles WHERE key=?1 AND id<>?2)")
        .await
        .map_err(storage_error)?
        .query_row(params![role.key.as_str(), role.id.to_string()])
        .await
        .map_err(storage_error)?
        .get::<bool>(0)
        .map_err(storage_error)?
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "role key is already in use",
        ));
    }
    if let Some(selection) = &role.model {
        let provider = super::agent::providers::get(db, selection.provider)
            .await?
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "role provider is unavailable"))?;
        validate_model(&provider, selection)?;
    }
    let mut role = role.clone();
    role.revision = revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("role revision exhausted"))?;
    db.execute(
        "INSERT INTO roles(id,key,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body",
        params![role.id.to_string(), role.key.as_str(), encode(&role)?],
    ).await.map_err(storage_error)?;
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
        if model
            .capabilities
            .as_ref()
            .and_then(|value| value.reasoning)
            == Some(false)
        {
            return Err(Fault::invalid("role reasoning effort is unavailable"));
        }
        model.validate_effort(provider.api, effort)?;
        effort.validate(provider.api, model.output)?;
    }
    Ok(())
}

pub(super) async fn remove(db: &Connection, id: RoleId, expected: u64) -> Result<(), Fault> {
    let role = get(db, id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "role does not exist"))?;
    if role.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "role revision changed",
        ));
    }
    db.execute("DELETE FROM roles WHERE id=?1", [id.to_string()])
        .await
        .map_err(storage_error)?;
    Ok(())
}
