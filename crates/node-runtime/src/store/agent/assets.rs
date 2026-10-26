//! Asset metadata is derived from the canonical history, never a second file index.
use super::*;
use sailry_protocol::conversation::assets::{self, Group, Kind, Query, Target};
mod extract;
#[cfg(test)]
mod tests;

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    query: &Query,
) -> Result<assets::Page, Fault> {
    check_session(db, session).await?;
    if !(1..=assets::MAX_GROUPS).contains(&query.limit)
        || query
            .before
            .is_some_and(|before| before == 0 || before > i64::MAX as u64)
    {
        return Err(sailry_protocol::Fault::invalid(
            "conversation asset bounds are invalid",
        ));
    }
    if let Some(before) = query.before {
        let visible: bool = {
            let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM conversation_events WHERE session=?1 AND sequence=?2)").await.map_err(storage_error)?;
            let row = statement
                .query_row(params![session.to_string(), before as i64])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        if !visible {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "asset cursor is not visible in this session",
            ));
        }
    }
    let mut statement = db
        .prepare(history::EVENTS_QUERY)
        .await
        .map_err(storage_error)?;
    let mut rows = statement
        .query(params![
            session.to_string(),
            query.before.unwrap_or(i64::MAX as u64) as i64,
            257
        ])
        .await
        .map_err(storage_error)?;
    let mut page = assets::Page {
        session,
        revision: crate::store::sessions::history_revision(db, session).await?,
        groups: Vec::new(),
        next_before: None,
    };
    let mut scanned = 0;
    let mut bytes = 0;
    let mut last = None;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        if scanned == 256
            || bytes >= 4 * 1024 * 1024
            || page.groups.len() == usize::from(query.limit)
        {
            page.next_before = last;
            break;
        }
        let sequence = row.get::<i64>(0).map_err(storage_error)? as u64;
        let turn: String = row.get(1).map_err(storage_error)?;
        let turn = turn.parse().map_err(storage_error)?;
        let body: Vec<u8> = row.get(2).map_err(storage_error)?;
        bytes += body.len();
        scanned += 1;
        last = Some(sequence);
        let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
        let kind = if event.author == "user" {
            Kind::Resource
        } else {
            Kind::Artifact
        };
        if query.kind.is_some_and(|filter| filter != kind) {
            continue;
        }
        let entry = history::project(db, sequence, turn, &event).await?;
        let run = runs::visible(db, session, turn).await?;
        let root: Option<String> = {
            let mut statement = db
                .prepare("SELECT path FROM worktrees WHERE id=?1")
                .await
                .map_err(storage_error)?;
            match statement.query_row([run.worktree.to_string()]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        let mut items = extract::items(&entry, root.as_deref());
        extract::publications(&mut items, &event);
        if !items.is_empty() {
            page.groups.push(Group {
                sequence,
                turn,
                worktree: run.worktree,
                timestamp_ms: entry.timestamp_ms,
                kind,
                items,
            });
        }
    }
    Ok(page)
}
