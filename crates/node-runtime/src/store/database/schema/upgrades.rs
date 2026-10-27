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
    Ok(crate::store::roles::needs_refresh(connection).await?)
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
    Ok(())
}
