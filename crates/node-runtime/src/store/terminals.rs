use super::{
    Ingress,
    database::{Database, encode, storage_error},
};
pub(super) mod settings;
use sailry_link::{Admission, Response};
use sailry_protocol::{
    terminal::{Info, Status},
    *,
};
use tokio::sync::{broadcast, oneshot};
use turso::{Connection, params};

pub(super) struct Opening {
    pub info: Info,
    pub root: std::path::PathBuf,
    pub settings: terminal::Settings,
}

impl Database {
    pub(super) async fn terminal_opening(&self, id: TerminalId) -> Result<Opening, Fault> {
        let info = required(&self.connection, id).await?;
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
            root: self.worktree_root(worktree).await?,
            settings: settings::read(&self.connection).await?,
        })
    }
}

pub(super) async fn required(db: &Connection, id: TerminalId) -> Result<Info, Fault> {
    read(db, id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "terminal does not exist"))
}

async fn read(db: &Connection, id: TerminalId) -> Result<Option<Info>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM terminals WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

impl Ingress {
    pub(super) async fn terminal(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
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

pub(super) async fn list(db: &Connection) -> Result<Vec<Info>, Fault> {
    {
        let mut rows = db
            .prepare("SELECT body FROM terminals ORDER BY id")
            .await
            .map_err(storage_error)?
            .query(())
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
    .collect()
}

pub(super) async fn record(db: &Connection, info: &Info) -> Result<Option<Event>, Fault> {
    record_update(db, info, false).await
}

// The PTY sends ordered metadata events. Command completions can arrive later
// with older metadata at the same controller revision and activity sequence.
async fn record_update(db: &Connection, info: &Info, event: bool) -> Result<Option<Event>, Fault> {
    let previous: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM terminals WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([info.id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
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
    db.execute("INSERT INTO terminals(id,worktree,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![info.id.to_string(), info.worktree.map(|id| id.to_string()), encode(info)?]).await.map_err(storage_error)?;
    Ok(Some(Event::TerminalChanged(info.clone())))
}

pub(super) async fn finish(
    db: &Connection,
    command: &Command,
    result: &mut Response,
) -> Result<Option<Event>, Fault> {
    match (command, &*result) {
        (
            Command::CloseTerminal { terminal, .. } | Command::CloseSshTerminal { terminal },
            Err(error),
        ) if error.code == ErrorCode::NotFound => {
            let body: Option<Vec<u8>> = {
                let mut statement = db
                    .prepare("SELECT body FROM terminals WHERE id=?1")
                    .await
                    .map_err(storage_error)?;
                match statement.query_row(params![terminal.to_string()]).await {
                    Ok(row) => {
                        let mapped: turso::Result<_> = row.get(0);
                        Some(mapped.map_err(storage_error)?)
                    }
                    Err(turso::Error::QueryReturnedNoRows) => None,
                    Err(error) => return Err(storage_error(error)),
                }
            };
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
            record(db, info).await
        }
        _ => Ok(None),
    }
}

impl Database {
    pub(super) async fn terminal_changed(
        &mut self,
        info: Info,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<(), Fault> {
        let write = self.write().await?;
        for attempt in 0..=super::database::conflict::RETRIES {
            let result = self.terminal_changed_once(&info, events).await;
            if let Err(error) = super::database::conflict::rollback(&self.connection).await {
                self.connection.hold_write(write);
                return Err(error);
            }
            if !super::database::conflict::again(&result, attempt).await {
                return result;
            }
        }
        unreachable!("the final attempt returns its result")
    }

    async fn terminal_changed_once(
        &self,
        info: &Info,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<(), Fault> {
        let publication = self.publication.clone();
        let transaction = super::database::conflict::begin(&self.connection).await?;
        let previous = read(&transaction, info.id).await?;
        let envelope = if let Some(event) = record_update(&transaction, info, true).await? {
            super::database::record(&transaction, self.node, vec![event])
                .await?
                .pop()
        } else {
            None
        };
        let notice = if envelope.is_some() {
            (match previous.as_ref() {
                Some(value) => Some(
                    async {
                        let previous = value;
                        super::notifications::system::terminal(&transaction, previous, info).await
                    }
                    .await,
                ),
                None => None,
            })
            .transpose()?
            .flatten()
        } else {
            None
        };
        let notices =
            super::database::record(&transaction, self.node, notice.into_iter().collect()).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        if let Some(event) = envelope {
            publication.send(events, event);
        }
        for notice in notices {
            publication.send(events, notice);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
