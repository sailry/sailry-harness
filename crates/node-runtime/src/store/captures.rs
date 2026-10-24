//! Immutable turn captures are loaded separately from the Node's admission index.
use sailry_protocol::{ErrorCode, Fault, QueuedTurn, SessionId, TurnId, TurnSummary};
use turso::Connection;

use super::database::storage_error;

pub(super) async fn list(db: &Connection) -> Result<Vec<TurnSummary>, Fault> {
    let mut query = db.prepare(
        "SELECT t.id,t.session,t.request FROM turns t JOIN sessions s ON s.id=t.session WHERE s.state!='removed' ORDER BY t.rowid",
    ).await.map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<String>(0)?,
                    row.get::<String>(1)?,
                    row.get::<String>(2)?,
                ))
            })(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
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

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<QueuedTurn, Fault> {
    super::commands::session(db, session).await?;
    super::agent::visible_run(db, session, turn).await?;
    let row = {
        let mut statement = db.prepare("SELECT t.session,t.request,t.revision,r.config,t.kind FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1").await.map_err(storage_error)?;
        match statement.query_row([turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| {
                    Ok((
                        row.get::<String>(0)?,
                        row.get::<String>(1)?,
                        row.get::<i64>(2)?,
                        row.get::<Vec<u8>>(3)?,
                        row.get::<String>(4)?,
                    ))
                })(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
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
        roles: super::sessions::roles::for_turn(db, turn).await?,
        plugins: super::agent::plugins::read(db, turn).await?,
    })
}

pub(in crate::store) async fn instructions(
    db: &Connection,
    turn: TurnId,
) -> Result<Vec<Instruction>, Fault> {
    let bytes: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT instructions FROM turns WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    serde_json::from_slice(&bytes).map_err(storage_error)
}

/// Frozen expanded text is private execution metadata, not another history.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Instruction {
    pub text: String,
    pub any_tools: Vec<String>,
}
