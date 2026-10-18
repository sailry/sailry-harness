//! Immutable turn captures are loaded separately from the Node's admission index.
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{ErrorCode, Fault, QueuedTurn, SessionId, TurnId, TurnSummary};

use super::database::storage_error;

pub(super) fn list(db: &Connection) -> Result<Vec<TurnSummary>, Fault> {
    let mut query = db.prepare(
        "SELECT t.id,t.session,t.request FROM turns t JOIN sessions s ON s.id=t.session WHERE s.state!='removed' ORDER BY t.rowid",
    ).map_err(storage_error)?;
    query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (id, session, request) = row.map_err(storage_error)?;
            Ok(TurnSummary {
                id: id.parse().map_err(storage_error)?,
                session: session.parse().map_err(storage_error)?,
                request: request.parse().map_err(storage_error)?,
            })
        })
        .collect()
}

pub(super) fn read(db: &Connection, session: SessionId, turn: TurnId) -> Result<QueuedTurn, Fault> {
    super::commands::session(db, session)?;
    super::agent::visible_run(db, session, turn)?;
    let row = db.query_row(
        "SELECT t.session,t.request,t.revision,r.config,t.kind FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1",
        [turn.to_string()],
        |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, i64>(2)?,row.get::<_, Vec<u8>>(3)?,row.get::<_, String>(4)?)),
    ).optional().map_err(storage_error)?;
    let (owner, request, revision, config, kind) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn capture does not exist"))?;
    let owner = owner.parse().map_err(storage_error)?;
    let revision = revision.try_into().map_err(storage_error)?;
    Ok(QueuedTurn {
        id: turn,
        session: owner,
        request: request.parse().map_err(storage_error)?,
        revision,
        kind: match kind.as_str() {
            "task" => sailry_protocol::conversation::RunKind::Task,
            "compaction" => sailry_protocol::conversation::RunKind::Compaction,
            _ => return Err(storage_error("turn kind is invalid")),
        },
        config: serde_json::from_slice(&config).map_err(storage_error)?,
        roles: super::sessions::roles::read(db, owner, revision)?,
        plugins: super::agent::plugins::read(db, turn)?,
    })
}
