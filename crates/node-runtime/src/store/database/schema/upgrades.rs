//! Defaults for added storage fields; no historical state inference.
mod json;
use crate::Error;
use rusqlite::Connection;

pub(super) fn column(table: &str, name: &str) -> Option<&'static [u8]> {
    match (table, name) {
        ("agent_approvals", "binding") => Some(b"null"),
        ("agent_runs", "hooks") => Some(
            br#"{"revision":0,"enabled":false,"before_tool":null,"after_tool":null,"turn_end":null}"#,
        ),
        _ => None,
    }
}

pub(super) fn retained(kind: &str, name: &str) -> bool {
    kind == "table" && name == "session_delegations"
}

pub(super) fn reconcile(db: &Connection) -> Result<bool, Error> {
    let changed = json::reconcile(db)?;
    crate::store::plugins::storage::search::rebuild(db)
        .map_err(|error| Error::Worker(error.message))?;
    crate::store::plugins::storage::search::validate(db)
        .map_err(|error| Error::Worker(error.message))?;
    Ok(changed)
}
