//! Logical pages use turn boundaries; large turn bodies use bounded continuations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
    before: Option<TurnId>,
    limit: u16,
) -> Result<History, Fault> {
    check_session(db, session)?;
    check_limit(limit)?;
    let boundary = before
        .map(|turn| runs::visible(db, session, turn).map(|run| run.sequence))
        .transpose()?
        .unwrap_or(i64::MAX as u64);
    let mut query = db
        .prepare(
            "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn
         JOIN conversation_turns h ON h.turn=a.turn
         WHERE h.session=?1 AND t.rowid<?2 AND json_extract(a.body,'$.status')!='queued'
           AND (json_extract(a.body,'$.status')!='cancelled'
                OR EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn))
         ORDER BY t.rowid DESC LIMIT ?3",
        )
        .map_err(storage_error)?;
    let mut runs = query
        .query_map(
            params![session.to_string(), boundary as i64, i64::from(limit) + 1],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?
        .map(|body| {
            serde_json::from_slice::<Run>(&body.map_err(storage_error)?)
                .map(|run| runs::project(run, session))
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let more = runs.len() > usize::from(limit);
    runs.truncate(usize::from(limit));
    let next_before = more.then(|| runs.last().expect("nonempty turn page").turn);
    if before.is_none() {
        // Preserve active and queue lifecycle metadata without counting it as older messages.
        let mut active = db.prepare(
            "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn
             JOIN conversation_turns h ON h.turn=a.turn
             WHERE h.session=?1 AND (json_extract(a.body,'$.status') IN ('running','stopping','queued')
                 OR (json_extract(a.body,'$.status')='cancelled'
                     AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn)))
             ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,t.rowid DESC
             LIMIT 100",
        ).map_err(storage_error)?;
        for row in active
            .query_map([session.to_string()], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
        {
            let run: Run =
                serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
            if !runs.iter().any(|current| current.turn == run.turn) {
                runs.push(runs::project(run, session));
            }
        }
    }
    runs.sort_by_key(|run| run.sequence);
    let turns: Vec<_> = runs.iter().map(|run| run.turn).collect();
    let (entries, _) = entries(db, session, &turns, None, 100)?;
    let mut page = page(db, session, runs, entries, next_before)?;
    fit(&mut page)?;
    let starts: BTreeMap<TurnId, u64> = {
        let mut query = db
            .prepare(
                "SELECT e.turn,MIN(e.sequence) FROM agent_events e JOIN conversation_turns h ON h.turn=e.turn
             WHERE h.session=?1 AND e.turn IN (SELECT value FROM json_each(?2)) GROUP BY e.turn",
            )
            .map_err(storage_error)?;
        query
            .query_map(params![session.to_string(), encode(&turns)?], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(storage_error)?
            .map(|row| {
                let (turn, sequence) = row.map_err(storage_error)?;
                Ok((turn.parse().map_err(storage_error)?, sequence as u64))
            })
            .collect::<Result<_, Fault>>()?
    };
    let missing = turns
        .into_iter()
        .filter(|turn| {
            starts.get(turn).is_some_and(|start| {
                !page
                    .entries
                    .iter()
                    .any(|entry| entry.turn == *turn && entry.sequence == *start)
            })
        })
        .collect();
    Ok(History {
        sequence: 0,
        page,
        missing,
    })
}

pub(in crate::store) fn turn(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<u64>,
    limit: u16,
) -> Result<TurnHistory, Fault> {
    check_limit(limit)?;
    if before.is_some_and(|value| value > i64::MAX as u64) {
        return Err(invalid("turn history cursor is invalid"));
    }
    let run = runs::visible(db, session, turn)?;
    let (entries, more) = entries(db, session, &[turn], before, limit)?;
    let count = entries.len();
    let mut page = page(db, session, vec![run.clone()], entries, None)?;
    fit(&mut page)?;
    let next_before = (more || page.entries.len() != count).then(|| {
        page.entries
            .first()
            .expect("nonempty turn continuation")
            .sequence
    });
    Ok(TurnHistory {
        sequence: 0,
        revision: page.revision,
        run,
        entries: page.entries,
        approvals: page.approvals,
        questions: page.questions,
        children: page.children,
        next_before,
    })
}

fn check_limit(limit: u16) -> Result<(), Fault> {
    if !(1..=100).contains(&limit) {
        return Err(invalid("history limit must be between one and one hundred"));
    }
    Ok(())
}

fn entries(
    db: &Connection,
    session: SessionId,
    turns: &[TurnId],
    before: Option<u64>,
    limit: u16,
) -> Result<(Vec<Entry>, bool), Fault> {
    let mut query = db
        .prepare(
            "SELECT e.sequence,e.turn,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND e.turn IN (SELECT value FROM json_each(?2)) AND h.sequence<?3
         ORDER BY h.sequence DESC LIMIT ?4",
        )
        .map_err(storage_error)?;
    let rows = query
        .query_map(
            params![
                session.to_string(),
                encode(&turns)?,
                before.unwrap_or(i64::MAX as u64) as i64,
                i64::from(limit) + 1
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(storage_error)?;
    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    for row in rows {
        let (sequence, turn, body) = row.map_err(storage_error)?;
        if entries.len() >= usize::from(limit)
            || !entries.is_empty() && bytes + body.len() > 3 * 1024 * 1024
        {
            more = true;
            break;
        }
        bytes += body.len();
        entries.push(project(
            db,
            sequence as u64,
            turn.parse().map_err(storage_error)?,
            &serde_json::from_slice(&body).map_err(storage_error)?,
        )?);
    }
    entries.reverse();
    Ok((entries, more))
}

fn page(
    db: &Connection,
    session: SessionId,
    runs: Vec<Run>,
    entries: Vec<Entry>,
    next_before: Option<TurnId>,
) -> Result<Page, Fault> {
    let turns: BTreeSet<_> = runs.iter().map(|run| run.turn).collect();
    let mut approvals = super::super::approvals::list(db, session, &entries)?;
    approvals.retain(|approval| turns.contains(&approval.turn));
    let mut questions = super::super::questions::list(db, session, &entries)?;
    questions.retain(|question| turns.contains(&question.turn));
    let children = super::super::children::list(db, &entries)?;
    Ok(Page {
        session,
        revision: crate::store::sessions::history_revision(db, session)?,
        entries,
        runs,
        next_before,
        approvals,
        questions,
        children,
        queue: super::super::queue::read(db, session)?,
    })
}

fn fit(page: &mut Page) -> Result<(), Fault> {
    while encode(page)?.len() > MAX_FRAME_BYTES - 64 * 1024 {
        if page.entries.len() <= 1 {
            return Err(invalid("conversation metadata exceeds the transport limit"));
        }
        page.entries.remove(0);
        retain_metadata(page);
    }
    Ok(())
}

pub(in crate::store::agent) fn retain_metadata(page: &mut Page) {
    let entries: BTreeSet<_> = page.entries.iter().map(|entry| entry.id.as_str()).collect();
    page.approvals.retain(|approval| {
        approval.state == ApprovalState::Pending || entries.contains(approval.entry.as_str())
    });
    page.questions.retain(|question| {
        question.state == question::State::Pending || entries.contains(question.entry.as_str())
    });
    page.children
        .retain(|child| entries.contains(child.origin.entry.as_str()));
}
