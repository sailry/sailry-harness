//! Defaults for added storage fields; no historical state inference.
mod instructions;
mod json;
mod local_models;
use crate::Error;
use rusqlite::Connection;
use sailry_protocol::hooks::Settings;
use std::sync::LazyLock;

static HOOKS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    serde_json::to_vec(&Settings::default()).expect("default hook settings encode")
});

pub(super) fn needed(db: &Connection) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM plugin_connections WHERE json_type(body,'$.login') IS NULL)",
        [],
        |row| row.get(0),
    )?)
}

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

pub(super) fn retained(kind: &str, name: &str) -> bool {
    kind == "table"
        && matches!(
            name,
            "session_delegations" | "ocr_settings" | "knowledge_settings" | "knowledge_models"
        )
}

pub(super) fn reconcile(db: &Connection, retain_instructions: bool) -> Result<bool, Error> {
    let models = local_models::reconcile(db)?;
    let changed = json::reconcile(db)?;
    let credentials = crate::store::databases::credentials::reconcile(db)
        .map_err(|error| Error::Worker(error.message))?;
    let instructions = retain_instructions && instructions::reconcile(db)?;
    // Added turn fields inherit only that turn's existing immutable revision.
    let captures=db.execute("UPDATE turns SET roles=(SELECT r.roles FROM session_revisions r WHERE r.session=turns.session AND r.revision=turns.revision) WHERE roles IS NULL",[])?;
    crate::store::plugins::storage::search::rebuild(db)
        .map_err(|error| Error::Worker(error.message))?;
    crate::store::plugins::storage::search::validate(db)
        .map_err(|error| Error::Worker(error.message))?;
    Ok(models || changed || credentials || instructions || captures != 0)
}
