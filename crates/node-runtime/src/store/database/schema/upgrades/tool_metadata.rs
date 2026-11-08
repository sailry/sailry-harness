use crate::{
    Error,
    agent::canonical::{EXECUTABLE_KEY, EXTERNAL_DISPATCH_KEY},
};
use serde_json::Value;
use turso::{Connection, params};

#[cfg(test)]
mod tests;

pub(super) async fn needed(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .prepare("SELECT EXISTS(SELECT 1 FROM node WHERE singleton=1 AND tool_metadata_names=0)")
        .await?
        .query_row(())
        .await?
        .get::<bool>(0)?)
}

pub(super) async fn apply(connection: &Connection) -> Result<(), Error> {
    if !needed(connection).await? {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT sequence,body FROM agent_events ORDER BY sequence",
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let sequence = row.get::<i64>(0)?;
        let mut body: Value = serde_json::from_slice(&row.get::<Vec<u8>>(1)?).map_err(|_| {
            Error::Worker(format!(
                "invalid conversation record {sequence}; existing data was not changed"
            ))
        })?;
        if record(&mut body)? {
            let body = serde_json::to_vec(&body)
                .map_err(|_| Error::Worker("conversation metadata could not be encoded".into()))?;
            connection
                .execute(
                    "UPDATE agent_events SET body=?1 WHERE sequence=?2",
                    params![body, sequence],
                )
                .await?;
        }
    }
    connection
        .execute(
            "UPDATE node SET tool_metadata_names=1 WHERE singleton=1",
            (),
        )
        .await?;
    Ok(())
}

fn record(body: &mut Value) -> Result<bool, Error> {
    match body.get("kind").and_then(Value::as_str) {
        Some("message") => message(body),
        Some("compaction") => body.get_mut("summary").map_or(Ok(false), message),
        Some("operation") if body.get("operation").and_then(Value::as_str) == Some("tool_meta") => {
            body.get_mut("patch").map_or(Ok(false), fields)
        }
        _ => Ok(false),
    }
}

fn message(body: &mut Value) -> Result<bool, Error> {
    let Some(content) = body.get_mut("content").and_then(Value::as_array_mut) else {
        return Ok(false);
    };
    let mut changed = false;
    for block in content {
        if block.get("type").and_then(Value::as_str) == Some("tool_request")
            && let Some(meta) = block.get_mut("tool_meta")
        {
            changed |= fields(meta)?;
        }
    }
    Ok(changed)
}

fn fields(meta: &mut Value) -> Result<bool, Error> {
    let Some(meta) = meta.as_object_mut() else {
        return Ok(false);
    };
    let mut changed = false;
    for (previous, current) in [
        ("goose.executable", EXECUTABLE_KEY),
        ("goose.external_dispatch", EXTERNAL_DISPATCH_KEY),
    ] {
        let Some(value) = meta.remove(previous) else {
            continue;
        };
        if meta.get(current).is_some_and(|current| current != &value) {
            return Err(Error::Worker(
                "conflicting tool execution metadata; existing data was not changed".into(),
            ));
        }
        meta.insert(current.into(), value);
        changed = true;
    }
    Ok(changed)
}
