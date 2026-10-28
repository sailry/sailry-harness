//! Derive model context from canonical membership, never wall-clock ordering.
use super::*;
use crate::agent::canonical::{BlockKind, Kind, Record, Role};

pub(crate) const END_EVENT: &str = "sailry.context.end_event";

pub(super) async fn validate(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    event: &Record,
) -> Result<(), Fault> {
    let Kind::Compaction(summary) = &event.kind else {
        return Ok(());
    };
    let end = &summary.boundary;
    if event.provenance.author != "system"
        || summary.summary.role != Role::User
        || summary.summary.content.is_empty()
        || summary
            .summary
            .content
            .iter()
            .any(|block| !matches!(&block.kind,BlockKind::Text { text } if !text.trim().is_empty()))
    {
        return Err(Fault::invalid("context summary content is invalid"));
    }
    let visible = history::VISIBLE_EVENT;
    let later_visible = visible.replace("e.body", "next.body");
    let valid: bool = {
        let mut statement = db.prepare(&format!("SELECT EXISTS(SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND e.turn<>?3 AND json_extract(r.body,'$.status') NOT IN ('queued','running','stopping')
         AND {visible}
         AND NOT EXISTS(SELECT 1 FROM conversation_events later JOIN agent_events next ON next.sequence=later.sequence
           WHERE later.session=h.session AND next.turn=e.turn AND later.sequence>h.sequence
           AND json_extract(next.body,'$.kind')!='compaction' AND {later_visible}))")).await.map_err(storage_error)?;
        let row = statement
            .query_row(params![session.to_string(), end.as_str(), turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if !valid && !current_exchange(db, session, turn, end).await? {
        return Err(sailry_protocol::Fault::invalid(
            "context summary boundary is not a complete turn or tool exchange",
        ));
    }
    Ok(())
}

async fn current_exchange(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    end: &str,
) -> Result<bool, Fault> {
    let visible = history::VISIBLE_EVENT;
    let boundary: Option<(i64, String)> = {
        let mut statement = db.prepare(&format!("SELECT e.sequence,e.turn FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND t.kind='task' AND {visible}
         AND ((e.turn=?3 AND json_extract(r.body,'$.status')='running')
           OR json_extract(r.body,'$.status') NOT IN ('queued','running','stopping'))")).await.map_err(storage_error)?;
        match statement
            .query_row(params![session.to_string(), end, turn.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some((sequence, owner)) = boundary else {
        return Ok(false);
    };
    let mut statement = db
        .prepare(
            "SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
         AND json_extract(body,'$.kind')!='compaction' ORDER BY sequence",
        )
        .await
        .map_err(storage_error)?;
    let bodies = {
        let mut rows = statement
            .query(params![owner, sequence])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            values.push(row.get::<Vec<u8>>(0).map_err(storage_error)?);
        }
        values
    };
    let mut events = Vec::with_capacity(bodies.len());
    for body in bodies {
        let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
        events.push(record);
    }
    Ok(complete_exchange(&events))
}

#[derive(Default)]
pub(crate) struct Exchange {
    pending: std::collections::HashMap<(Option<String>, String), usize>,
    invalid: bool,
    complete: bool,
}

impl Exchange {
    pub(crate) fn push(&mut self, event: &Record) -> bool {
        let Some(message) = event.message() else {
            return self.complete;
        };
        let mut response = false;
        for block in &message.content {
            match &block.kind {
                BlockKind::ToolRequest(call) => {
                    *self
                        .pending
                        .entry((call.id.clone(), call.name.clone()))
                        .or_default() += 1;
                }
                BlockKind::ToolResponse(result) => {
                    response = true;
                    let key = (result.id.clone(), result.name.clone());
                    if let Some(count) = self.pending.get_mut(&key) {
                        *count -= 1;
                        if *count == 0 {
                            self.pending.remove(&key);
                        }
                    } else {
                        self.invalid = true;
                    }
                }
                _ => {}
            }
        }
        self.complete = !self.invalid && response && self.pending.is_empty();
        self.complete
    }
}

pub(crate) fn complete_exchange(events: &[Record]) -> bool {
    let mut exchange = Exchange::default();
    let mut complete = false;
    for event in events {
        complete = exchange.push(event);
    }
    complete
}

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Option<(i64, Record)>, Fault> {
    let mut rows = db.query("SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND json_extract(e.body,'$.kind')='compaction' ORDER BY h.sequence DESC LIMIT 1",[session.to_string()]).await.map_err(storage_error)?;
    let Some(row) = rows.next().await.map_err(storage_error)? else {
        return Ok(None);
    };
    let body: Vec<u8> = row.get(0).map_err(storage_error)?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let Kind::Compaction(summary) = &record.kind else {
        unreachable!("selected compaction record");
    };
    let mut rows = db.query("SELECT h.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2", params![session.to_string(),summary.boundary.as_str()]).await.map_err(storage_error)?;
    let row = rows
        .next()
        .await
        .map_err(storage_error)?
        .ok_or_else(|| Fault::invalid("compaction boundary is missing"))?;
    Ok(Some((row.get(0).map_err(storage_error)?, record)))
}

pub(crate) struct History {
    pub events: Vec<Record>,
    pub completed: Vec<usize>,
    pub current: usize,
    pub recent: Option<usize>,
}

pub(super) async fn history(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<History, Fault> {
    let snapshot = sessions::get(
        db,
        turn,
        sessions::Query {
            session,
            recent: None,
            after: None,
        },
    )
    .await?;
    let current = snapshot
        .records
        .iter()
        .position(|record| record.turn == turn)
        .unwrap_or(snapshot.records.len());
    let mut completed = Vec::new();
    let mut starts = Vec::new();
    let mut previous = None;
    let mut exchange = Exchange::default();
    for (index, event) in snapshot.records.iter().enumerate() {
        if event.message().is_none() {
            continue;
        }
        if previous != Some(event.turn) {
            if previous.is_some() {
                completed.push(index);
            }
            previous = Some(event.turn);
            exchange = Exchange::default();
            starts.push(index);
        }
        if exchange.push(event) {
            completed.push(index + 1);
        }
    }
    if let Some(last) = snapshot.records.last()
        && last.turn != turn
    {
        completed.push(snapshot.records.len());
    }
    completed.sort_unstable();
    completed.dedup();
    Ok(History {
        events: snapshot.records,
        completed,
        current,
        recent: starts.iter().rev().nth(1).copied(),
    })
}
