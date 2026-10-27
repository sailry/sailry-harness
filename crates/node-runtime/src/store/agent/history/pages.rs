//! Logical pages use turn boundaries; large turn bodies use bounded continuations.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(test)]
mod tests;

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    before: Option<TurnId>,
    limit: u16,
) -> Result<History, Fault> {
    check_session(db, session).await?;
    let limit = bounded_limit(limit)?;
    let boundary = match before {
        Some(turn) => runs::visible(db, session, turn).await?.sequence,
        None => i64::MAX as u64,
    };
    let mut query = db
        .prepare(
            "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn
         JOIN conversation_turns h ON h.turn=a.turn
         WHERE h.session=?1 AND t.rowid<?2 AND json_extract(a.body,'$.status')!='queued'
           AND (json_extract(a.body,'$.status')!='cancelled'
                OR a.input IS NOT NULL
                OR EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn))
         ORDER BY t.rowid DESC LIMIT ?3",
        )
        .await
        .map_err(storage_error)?;
    let mut runs = {
        let mut rows = query
            .query(params![
                session.to_string(),
                boundary as i64,
                i64::from(limit) + 1
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
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
                     AND a.input IS NULL
                     AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn)))
             ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,t.rowid DESC
             LIMIT 100",
        ).await.map_err(storage_error)?;
        {
            let mut rows = active
                .query([session.to_string()])
                .await
                .map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let row: turso::Result<_> = row.get::<Vec<u8>>(0);

                let run: Run =
                    serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
                if !runs.iter().any(|current| current.turn == run.turn) {
                    runs.push(runs::project(run, session));
                }
            }
        }
    }
    runs.sort_by_key(|run| run.sequence);
    let turns: Vec<_> = runs.iter().map(|run| run.turn).collect();
    let (entries, _) = entries(db, session, &turns, None, 100).await?;
    let mut page = page(db, session, runs, entries, next_before).await?;
    fit(&mut page, 0, true)?;
    let turns: Vec<_> = page.runs.iter().map(|run| run.turn).collect();
    let starts: BTreeMap<TurnId, u64> = {
        let mut query = db
            .prepare(
                &format!("SELECT e.turn,MIN(e.sequence) FROM agent_events e JOIN conversation_turns h ON h.turn=e.turn
             WHERE h.session=?1 AND e.turn IN (SELECT value FROM json_each(?2)) AND {VISIBLE_EVENT} GROUP BY e.turn"),
            ).await
            .map_err(storage_error)?;
        {
            let mut rows = query
                .query(params![session.to_string(), encode(&turns)?])
                .await
                .map_err(storage_error)?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<i64>(1)?)))(&row);
                values.push(mapped);
            }
            values.into_iter()
        }
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

pub(in crate::store) async fn turn(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<u64>,
    limit: u16,
) -> Result<TurnHistory, Fault> {
    let limit = bounded_limit(limit)?;
    if before.is_some_and(|value| value > i64::MAX as u64) {
        return Err(sailry_protocol::Fault::invalid(
            "turn history cursor is invalid",
        ));
    }
    let run = runs::visible(db, session, turn).await?;
    let (entries, more) = entries(db, session, &[turn], before, limit).await?;
    let count = entries.len();
    let mut page = page(db, session, vec![run.clone()], entries, None).await?;
    fit(&mut page, 0, false)?;
    let run = page.runs.pop().expect("one turn continuation");
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

fn bounded_limit(limit: u16) -> Result<u16, Fault> {
    if limit == 0 {
        return Err(sailry_protocol::Fault::invalid(
            "history limit must be positive",
        ));
    }
    Ok(limit.min(100))
}

async fn entries(
    db: &Connection,
    session: SessionId,
    turns: &[TurnId],
    before: Option<u64>,
    limit: u16,
) -> Result<(Vec<Entry>, bool), Fault> {
    let mut query = db
        .prepare(
            &format!("SELECT e.sequence,e.turn,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND e.turn IN (SELECT value FROM json_each(?2)) AND h.sequence<?3
         AND {VISIBLE_EVENT} ORDER BY h.sequence DESC LIMIT ?4"),
        ).await
        .map_err(storage_error)?;
    let rows = {
        let mut rows = query
            .query(params![
                session.to_string(),
                encode(&turns)?,
                before.unwrap_or(i64::MAX as u64) as i64,
                i64::from(limit) + 1
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<String>(1)?,
                    row.get::<Vec<u8>>(2)?,
                ))
            })(&row);
            values.push(mapped);
        }
        values.into_iter()
    };
    let mut events = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    for row in rows {
        let (sequence, turn, body) = row.map_err(storage_error)?;
        if events.len() >= usize::from(limit)
            || !events.is_empty() && bytes + body.len() > 3 * 1024 * 1024
        {
            more = true;
            break;
        }
        bytes += body.len();
        events.push((
            sequence as u64,
            turn.parse().map_err(storage_error)?,
            serde_json::from_slice(&body).map_err(storage_error)?,
        ));
    }
    let sources = tools::Sources::new(db, &events).await?;
    let mut entries = Vec::with_capacity(events.len());
    for (sequence, turn, event) in events {
        entries.push(project_with_sources(db, sequence, turn, &event, Some(&sources)).await?);
    }
    entries.reverse();
    Ok((entries, more))
}

async fn page(
    db: &Connection,
    session: SessionId,
    runs: Vec<Run>,
    entries: Vec<Entry>,
    next_before: Option<TurnId>,
) -> Result<Page, Fault> {
    let mut hydrated = Vec::with_capacity(runs.len());
    for run in runs {
        hydrated.push(runs::hydrate(db, run).await?);
    }
    let runs = hydrated;
    let turns: BTreeSet<_> = runs.iter().map(|run| run.turn).collect();
    let mut approvals = super::super::approvals::list(db, session, &entries).await?;
    approvals.retain(|approval| turns.contains(&approval.turn));
    let mut questions = super::super::questions::list(db, session, &entries).await?;
    questions.retain(|question| turns.contains(&question.turn));
    let children = super::super::children::list(db, &entries).await?;
    Ok(Page {
        background: super::super::background::list(db, session).await?,
        session,
        revision: crate::store::sessions::history_revision(db, session).await?,
        entries,
        runs,
        next_before,
        approvals,
        questions,
        children,
        queue: super::super::queue::read(db, session).await?,
    })
}

pub(in crate::store::agent) fn fit(
    page: &mut Page,
    headroom: usize,
    prune_runs: bool,
) -> Result<BTreeSet<TurnId>, Fault> {
    let budget = (MAX_FRAME_BYTES - 64 * 1024)
        .checked_sub(headroom)
        .ok_or_else(|| Fault::invalid("conversation resources exceed the transport limit"))?;
    let mut sizes = page
        .entries
        .iter()
        .map(|entry| Ok((entry.turn, encode(entry)?.len())))
        .collect::<Result<VecDeque<_>, Fault>>()?;
    let mut entries_bytes = sizes.iter().map(|(_, bytes)| bytes).sum::<usize>();
    let mut trimmed = BTreeSet::new();
    while encoded_size(page, entries_bytes)? > budget {
        if page.entries.len() > 1 {
            trimmed.insert(page.entries.remove(0).turn);
            entries_bytes -= sizes.pop_front().expect("cached entry size").1;
            retain_metadata(page);
            continue;
        }
        let closed: Vec<_> = page
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| {
                !matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
            })
            .map(|(index, run)| (index, run.turn))
            .collect();
        if !prune_runs || closed.len() <= 1 {
            return Err(Fault::invalid(
                "conversation metadata exceeds the transport limit",
            ));
        }
        let (index, turn) = closed[0];
        page.runs.remove(index);
        page.entries.retain(|entry| entry.turn != turn);
        sizes.retain(|(current, bytes)| {
            if *current == turn {
                entries_bytes -= *bytes;
                false
            } else {
                true
            }
        });
        page.approvals.retain(|approval| approval.turn != turn);
        page.questions.retain(|question| question.turn != turn);
        page.children.retain(|child| child.origin.turn != turn);
        page.next_before = Some(closed[1].1);
    }
    trimmed.retain(|turn| page.runs.iter().any(|run| run.turn == *turn));
    Ok(trimmed)
}

fn encoded_size(page: &mut Page, entries_bytes: usize) -> Result<usize, Fault> {
    // Retain exact metadata sizing while encoding each unchanged entry only once.
    let entries = std::mem::take(&mut page.entries);
    let metadata = encode(page);
    page.entries = entries;
    Ok(metadata?.len() + entries_bytes + page.entries.len().saturating_sub(1))
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
