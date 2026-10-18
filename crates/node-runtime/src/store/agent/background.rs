//! Completion evidence shares the storage actor and canonical conversation history.
use super::*;
use sailry_protocol::conversation::background::{Kind, Notice, Outcome, State, Task};
use serde::{Deserialize, Serialize};

mod delivery;
mod records;
#[cfg(test)]
mod tests;

pub(crate) use delivery::INPUT_IDS;
pub(in crate::store) use delivery::schedule;
pub(super) use delivery::{bind, check, confirm, render};
pub(super) use records::{child_finished, child_started, consume_child, recover, release};
pub(in crate::store) use records::{
    command_finished, command_started, consume_command, finish_request,
};
pub(in crate::store) use records::{dismiss, list};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Record {
    task: Task,
    stdout: Option<String>,
    stderr: Option<String>,
    response: Option<String>,
    elapsed_ms: Option<u64>,
    sequence: u64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn preview(text: &str, bytes: usize) -> String {
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= bytes)
        .unwrap_or(text.len());
    text[..end].to_owned()
}

fn tail(text: &str, bytes: usize) -> String {
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= text.len().saturating_sub(bytes))
        .unwrap_or(text.len());
    text[start..].to_owned()
}

fn get(db: &Connection, id: &str) -> Result<Option<Record>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_background WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

fn select(db: &Connection, sql: &str, value: &str) -> Result<Vec<Record>, Fault> {
    let mut query = db.prepare(sql).map_err(storage_error)?;
    query
        .query_map([value], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

fn save(db: &Connection, record: &mut Record) -> Result<(), Fault> {
    record.task.revision = record
        .task
        .revision
        .checked_add(1)
        .ok_or_else(|| storage_error("background revision exhausted"))?;
    let (kind, source) = match record.task.kind {
        Kind::Agent { session, .. } => ("agent", session.to_string()),
        Kind::Command { id } => ("command", id.to_string()),
    };
    let state = match record.task.state {
        State::Running => "running",
        State::Pending => "pending",
        State::Delivered => "delivered",
        State::Consumed => "consumed",
        State::Dismissed => "dismissed",
    };
    db.execute("INSERT INTO agent_background(id,session,turn,kind,source,state,delivery,sequence,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET state=excluded.state,delivery=excluded.delivery,sequence=excluded.sequence,body=excluded.body",
        params![record.task.id,record.task.session.to_string(),record.task.turn.to_string(),kind,source,state,record.task.delivery.map(|turn|turn.to_string()),record.sequence as i64,encode(record)?]).map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn publish(database: &Database, tasks: impl IntoIterator<Item = Task>) {
    for task in tasks {
        database
            .feeds
            .publish(database.node, task.session, Change::Background(task));
    }
}

fn next_sequence(db: &Connection) -> Result<u64, Fault> {
    db.query_row(
        "SELECT coalesce(max(sequence),0)+1 FROM agent_background",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value as u64)
    .map_err(storage_error)
}

impl Ingress {
    pub(crate) async fn collect_command(
        &self,
        parent: TurnId,
        snapshot: process::Snapshot,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::Agent(Box::new(Operation::ConsumeCommand {
            parent,
            snapshot,
            reply,
        })))
        .await?;
        response.await.map_err(|_| super::super::unavailable())?
    }
    pub(crate) async fn collect_child(
        &self,
        parent: TurnId,
        child: SessionId,
        turn: TurnId,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::Agent(Box::new(Operation::CollectChild {
            parent,
            child,
            turn,
            reply,
        })))
        .await?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}
