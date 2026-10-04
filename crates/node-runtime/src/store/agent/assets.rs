//! Asset metadata is derived from the canonical history, never a second file index.
use super::*;
use sailry_protocol::conversation::assets::{self, Group, Kind, Query, Target};
mod extract;
#[cfg(test)]
mod tests;

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
    query: &Query,
) -> Result<assets::Page, Fault> {
    check_session(db, session)?;
    if !(1..=assets::MAX_GROUPS).contains(&query.limit)
        || query
            .before
            .is_some_and(|before| before == 0 || before > i64::MAX as u64)
    {
        return Err(invalid("conversation asset bounds are invalid"));
    }
    if let Some(before) = query.before {
        let visible: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_events WHERE session=?1 AND sequence=?2)",
                params![session.to_string(), before as i64],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !visible {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "asset cursor is not visible in this session",
            ));
        }
    }
    let mut statement = db.prepare(history::EVENTS_QUERY).map_err(storage_error)?;
    let mut rows = statement
        .query(params![
            session.to_string(),
            query.before.unwrap_or(i64::MAX as u64) as i64,
            257
        ])
        .map_err(storage_error)?;
    let mut page = assets::Page {
        session,
        revision: crate::store::sessions::history_revision(db, session)?,
        groups: Vec::new(),
        next_before: None,
    };
    let mut scanned = 0;
    let mut bytes = 0;
    let mut last = None;
    while let Some(row) = rows.next().map_err(storage_error)? {
        if scanned == 256
            || bytes >= 4 * 1024 * 1024
            || page.groups.len() == usize::from(query.limit)
        {
            page.next_before = last;
            break;
        }
        let sequence = row.get::<_, i64>(0).map_err(storage_error)? as u64;
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
        let entry = history::project(db, sequence, turn, &event)?;
        let run = runs::visible(db, session, turn)?;
        let root: Option<String> = db
            .query_row(
                "SELECT path FROM worktrees WHERE id=?1",
                [run.worktree.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let items = extract::items(&entry, root.as_deref());
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
