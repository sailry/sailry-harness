//! Current-format defaults for added native storage fields.
use crate::Error;
use sailry_protocol::{artifact::Kind, hooks::Settings};
use std::sync::LazyLock;
use turso::Connection;

static HOOKS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    serde_json::to_vec(&Settings::default()).expect("default hook settings encode")
});

pub(super) fn column(table: &str, name: &str) -> Option<&'static [u8]> {
    match (table, name) {
        ("turns", "instructions") => Some(b"[]"),
        ("agent_approvals", "binding" | "prepared") => Some(b"null"),
        ("agent_runs", "hooks") => Some(HOOKS.as_slice()),
        _ => None,
    }
}

pub(super) fn matches(table: &str, name: &str, value: &[u8]) -> bool {
    match (table, name) {
        ("turns", "instructions") => value == b"[]",
        ("agent_runs", "hooks") => serde_json::from_slice::<Settings>(value)
            .is_ok_and(|settings| settings == Settings::default()),
        ("agent_approvals", "binding" | "prepared") => {
            serde_json::from_slice::<serde_json::Value>(value).is_ok_and(|value| value.is_null())
        }
        _ => false,
    }
}

const ARTIFACTS: [&str; 2] = ["artifacts", "artifact_revisions"];
const MISSING_KIND: &str = "CASE WHEN json_valid(body) THEN json_type(body)='object' AND json_type(body,'$.kind') IS NULL ELSE 0 END";

pub(super) async fn needed(connection: &Connection) -> Result<bool, Error> {
    for table in ARTIFACTS {
        if connection
            .prepare(format!(
                "SELECT EXISTS(SELECT 1 FROM {table} WHERE {MISSING_KIND})"
            ))
            .await?
            .query_row(())
            .await?
            .get::<bool>(0)?
        {
            return Ok(true);
        }
    }
    Ok(crate::store::roles::needs_refresh(connection).await? || tools_pending(connection).await?)
}

pub(super) async fn apply(connection: &Connection) -> Result<(), Error> {
    let kind = serde_json::to_value(Kind::default()).expect("default artifact kind encodes");
    let kind = kind.as_str().expect("artifact kind is a string");
    for table in ARTIFACTS {
        connection.execute(
            format!("UPDATE {table} SET body=CAST(json_set(body,'$.kind',?1) AS BLOB) WHERE {MISSING_KIND}"),
            [kind],
        ).await?;
    }
    crate::store::roles::refresh(connection).await?;
    if tools_pending(connection).await? {
        // Existing arrays may contain discoveries alone; union historical calls once.
        for (session, names) in crate::store::agent::history::missing_tools(connection)
            .await
            .map_err(|error| Error::Worker(error.message))?
        {
            crate::store::agent::history::remember(connection, session, names)
                .await
                .map_err(|error| Error::Worker(error.message))?;
        }
        connection
            .execute("UPDATE node SET tool_defaults=1 WHERE singleton=1", ())
            .await?;
    }
    Ok(())
}

async fn tools_pending(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .prepare("SELECT EXISTS(SELECT 1 FROM node WHERE singleton=1 AND tool_defaults=0)")
        .await?
        .query_row(())
        .await?
        .get::<bool>(0)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completed_tools_skip_history() {
        let directory = tempfile::tempdir().unwrap();
        let database = crate::store::database::engine(&directory.path().join("node.turso"))
            .await
            .unwrap();
        let connection = crate::store::database::connect(&database).await.unwrap();
        // History tables are unnecessary after the persisted default converges.
        connection
            .execute_batch(
                "CREATE TABLE node(singleton INTEGER PRIMARY KEY,tool_defaults INTEGER);
                 INSERT INTO node VALUES(1,1);
                 CREATE TABLE artifacts(body BLOB);
                 CREATE TABLE artifact_revisions(body BLOB);
                 CREATE TABLE roles(key TEXT,body BLOB);",
            )
            .await
            .unwrap();
        assert!(!needed(&connection).await.unwrap());
        apply(&connection).await.unwrap();
        connection
            .execute("UPDATE node SET tool_defaults=0", ())
            .await
            .unwrap();
        assert!(needed(&connection).await.unwrap());
        assert!(apply(&connection).await.is_err());
        assert!(tools_pending(&connection).await.unwrap());
    }
}
