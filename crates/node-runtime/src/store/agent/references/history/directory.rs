//! A bounded view of the existing session directory, without execution configuration.
use super::*;
use serde::Serialize;

const MAX_SCANNED: usize = 256;

#[derive(Serialize)]
struct Entry {
    session: SessionId,
    title: String,
    project: Option<ProjectId>,
    worktree: WorktreeId,
    archived: bool,
    creation_order: u64,
    history_revision: u64,
    turn: Option<TurnId>,
    status: Option<Status>,
    updated_ms: Option<i64>,
}

#[derive(Serialize)]
struct Page {
    node: NodeId,
    project: Option<ProjectId>,
    revision: String,
    sessions: Vec<Entry>,
    next_before: Option<u64>,
}

pub(super) fn read(
    db: &Connection,
    node: NodeId,
    source: &Session,
    options: List,
) -> Result<Value, Fault> {
    let limit = options.limit.unwrap_or(20);
    if !(1..=100).contains(&limit)
        || options.text.as_ref().is_some_and(|text| text.len() > 512)
        || options
            .before
            .is_some_and(|value| value == 0 || value > i64::MAX as u64)
        || matches!((options.since_ms, options.until_ms), (Some(since), Some(until)) if since > until)
    {
        return Err(Fault::invalid("session directory bounds are invalid"));
    }
    let revision = revision(db, source, options.include_current)?;
    if options
        .revision
        .as_ref()
        .is_some_and(|expected| *expected != revision)
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "session directory changed",
        ));
    }
    if let Some(before) = options.before {
        let id: Option<String> = db
            .query_row(
                "SELECT id FROM sessions WHERE rowid=?1 AND state!='removed'",
                [before as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let id =
            id.ok_or_else(|| Fault::new(ErrorCode::NotFound, "session cursor is unavailable"))?;
        visible(db, source.project, id.parse().map_err(storage_error)?)?;
    }
    let mut query = db
        .prepare(
            "SELECT rowid,id,worktree,state,history_revision FROM sessions
             WHERE project IS ?1 AND state!='removed' AND rowid<?2
               AND (?3 OR id!=?4)
             ORDER BY rowid DESC LIMIT ?5",
        )
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![
            source.project.map(|id| id.to_string()),
            options.before.unwrap_or(i64::MAX as u64) as i64,
            options.include_current,
            source.id.to_string(),
            (MAX_SCANNED + 1) as i64,
        ])
        .map_err(storage_error)?;
    let text = options.text.map(|text| text.trim().to_lowercase());
    let mut page = Page {
        node,
        project: source.project,
        revision,
        sessions: Vec::new(),
        next_before: None,
    };
    let mut scanned = 0;
    let mut last = None;
    while let Some(row) = rows.next().map_err(storage_error)? {
        if scanned == MAX_SCANNED || page.sessions.len() == usize::from(limit) {
            page.next_before = last;
            break;
        }
        scanned += 1;
        let creation_order = row.get::<_, i64>(0).map_err(storage_error)? as u64;
        last = Some(creation_order);
        let id: SessionId = row
            .get::<_, String>(1)
            .map_err(storage_error)?
            .parse()
            .map_err(storage_error)?;
        let activity = activity::read(db, id)?;
        let updated_ms = activity
            .run
            .as_ref()
            .and_then(|run| run.finished_ms.or(run.started_ms));
        if text
            .as_ref()
            .is_some_and(|text| !activity.title.to_lowercase().contains(text))
            || options
                .since_ms
                .is_some_and(|since| updated_ms.is_none_or(|time| time < since))
            || options
                .until_ms
                .is_some_and(|until| updated_ms.is_none_or(|time| time > until))
        {
            continue;
        }
        page.sessions.push(Entry {
            session: id,
            title: activity.title,
            project: source.project,
            worktree: row
                .get::<_, String>(2)
                .map_err(storage_error)?
                .parse()
                .map_err(storage_error)?,
            archived: row.get::<_, String>(3).map_err(storage_error)? == "archived",
            creation_order,
            history_revision: row.get::<_, i64>(4).map_err(storage_error)? as u64,
            turn: activity.run.as_ref().map(|run| run.turn),
            status: activity.run.as_ref().map(|run| run.status),
            updated_ms,
        });
    }
    serde_json::to_value(page).map_err(storage_error)
}

fn revision(db: &Connection, source: &Session, include_current: bool) -> Result<String, Fault> {
    let mut query = db
        .prepare(
            "SELECT s.rowid,s.id,s.worktree,s.state,s.history_revision,
            (SELECT body FROM agent_state WHERE owner=s.id AND key=?3),
            (SELECT a.body FROM conversation_turns h JOIN turns t ON t.id=h.turn
             JOIN agent_runs a ON a.turn=t.id WHERE h.session=s.id
             ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,
                (json_extract(a.body,'$.status')='queued'),t.rowid DESC LIMIT 1)
         FROM sessions s WHERE s.project IS ?1 AND (?2 OR s.id!=?4) ORDER BY s.rowid",
        )
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![
            source.project.map(|id| id.to_string()),
            include_current,
            SESSION_TITLE,
            source.id.to_string()
        ])
        .map_err(storage_error)?;
    let mut hasher = blake3::Hasher::new();
    while let Some(row) = rows.next().map_err(storage_error)? {
        let body = row.get::<_, Option<Vec<u8>>>(6).map_err(storage_error)?;
        let run = body
            .map(|body| serde_json::from_slice::<Run>(&body).map_err(storage_error))
            .transpose()?;
        let metadata = (
            row.get::<_, i64>(0).map_err(storage_error)?,
            row.get::<_, String>(1).map_err(storage_error)?,
            row.get::<_, String>(2).map_err(storage_error)?,
            row.get::<_, String>(3).map_err(storage_error)?,
            row.get::<_, i64>(4).map_err(storage_error)?,
            row.get::<_, Option<Vec<u8>>>(5).map_err(storage_error)?,
            run.map(|run| (run.turn, run.status, run.started_ms, run.finished_ms)),
        );
        hasher.update(&serde_json::to_vec(&metadata).map_err(storage_error)?);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
