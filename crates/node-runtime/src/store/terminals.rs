use super::{
    Ingress,
    database::{Database, encode, storage_error},
};
pub(super) mod settings;
use rusqlite::{Connection, OptionalExtension, params};
use sailry_link::{Admission, Response};
use sailry_protocol::{
    terminal::{Info, Status},
    *,
};
use tokio::sync::{broadcast, oneshot};

pub(super) struct Opening {
    pub info: Info,
    pub root: std::path::PathBuf,
    pub settings: terminal::Settings,
}

impl Database {
    pub(super) fn terminal_opening(&self, id: TerminalId) -> Result<Opening, Fault> {
        let info = required(&self.connection, id)?;
        if info.status == Status::Closed {
            return Err(Fault::new(ErrorCode::NotFound, "terminal has been closed"));
        }
        if info.ssh.is_some() || info.tool.is_some() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "terminal requires an explicit new launch",
            ));
        }
        let worktree = info
            .worktree
            .ok_or_else(|| Fault::new(ErrorCode::WrongTarget, "terminal has no worktree"))?;
        Ok(Opening {
            info,
            root: self.worktree_root(worktree)?,
            settings: settings::read(&self.connection)?,
        })
    }
}

pub(super) fn required(db: &Connection, id: TerminalId) -> Result<Info, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM terminals WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "terminal does not exist"))?,
    )
    .map_err(storage_error)
}

impl Ingress {
    pub(super) async fn terminal(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = self.terminals.command(caller, request.command).await;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(output);
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}

pub(super) fn list(db: &Connection) -> Result<Vec<Info>, Fault> {
    db.prepare("SELECT body FROM terminals ORDER BY id")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

pub(super) fn record(db: &Connection, info: &Info) -> Result<Option<Event>, Fault> {
    record_update(db, info, false)
}

// The PTY sends ordered metadata events. Command completions can arrive later
// with older metadata at the same controller revision and activity sequence.
fn record_update(db: &Connection, info: &Info, event: bool) -> Result<Option<Event>, Fault> {
    let previous: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM terminals WHERE id=?1",
            [info.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(previous) = previous {
        let previous: Info = serde_json::from_slice(&previous).map_err(storage_error)?;
        let before = previous.activity.map_or(0, |report| report.sequence);
        let after = info.activity.map_or(0, |report| report.sequence);
        if previous == *info
            || previous.revision > info.revision
            || (previous.revision == info.revision
                && (before > after || (before == after && !event)))
        {
            return Ok(None);
        }
    }
    db.execute("INSERT INTO terminals(id,worktree,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![info.id.to_string(), info.worktree.map(|id| id.to_string()), encode(info)?]).map_err(storage_error)?;
    Ok(Some(Event::TerminalChanged(info.clone())))
}

pub(super) fn finish(
    db: &Connection,
    command: &Command,
    result: &mut Response,
) -> Result<Option<Event>, Fault> {
    match (command, &*result) {
        (
            Command::CloseTerminal { terminal, .. } | Command::CloseSshTerminal { terminal },
            Err(error),
        ) if error.code == ErrorCode::NotFound => {
            let body: Option<Vec<u8>> = db
                .query_row(
                    "SELECT body FROM terminals WHERE id=?1",
                    params![terminal.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if let Some(body) = body {
                let mut info: Info = serde_json::from_slice(&body).map_err(storage_error)?;
                let matches = match command {
                    Command::CloseTerminal { worktree, .. } => info.worktree == Some(*worktree),
                    Command::CloseSshTerminal { .. } => info.ssh.is_some(),
                    _ => false,
                };
                if matches && info.status != Status::Running {
                    info.status = Status::Closed;
                    info.owner = None;
                    info.revision += 1;
                    *result = Ok(Output::Terminal(info));
                }
            }
        }
        _ => {}
    }
    match &*result {
        Ok(Output::Terminal(info) | Output::SshOutcome(ssh::Outcome::Terminal(info))) => {
            record(db, info)
        }
        _ => Ok(None),
    }
}

impl Database {
    pub(super) fn terminal_changed(
        &mut self,
        info: Info,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<(), Fault> {
        let transaction = self.connection.transaction().map_err(storage_error)?;
        let envelope = if let Some(event) = record_update(&transaction, &info, true)? {
            transaction
                .execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
                .map_err(storage_error)?;
            Some(EventEnvelope {
                node: self.node,
                cursor: transaction.last_insert_rowid() as u64,
                event,
            })
        } else {
            None
        };
        transaction.commit().map_err(storage_error)?;
        if let Some(event) = envelope {
            let _ = events.send(event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
