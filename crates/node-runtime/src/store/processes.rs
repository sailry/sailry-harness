//! Managed commands share durable admission; waiting never occupies the file mutation worker.
use super::{
    CAPACITY, Job,
    database::{Database, storage_error},
    external::Completed,
};
use crate::process::{self, Environment, Launch};
use futures::FutureExt;
use rusqlite::OptionalExtension;
use sailry_link::{CancellationToken, Response};
use sailry_protocol::*;
use std::{panic::AssertUnwindSafe, sync::Arc};
use tokio::{
    runtime::Handle,
    sync::{Semaphore, mpsc, oneshot},
};

pub(super) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub launch: Prepared,
    pub reply: oneshot::Sender<Response>,
}

pub(super) struct Prepared {
    launch: Launch,
    session: SessionId,
    background: bool,
}

pub(super) fn prepare(database: &Database, request: &Request) -> Result<Prepared, Fault> {
    let Command::RunCommand {
        turn,
        command,
        cwd,
        timeout_ms,
        background,
        attachments,
    } = &request.command
    else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "command execution expected",
        ));
    };
    let row: Option<(String, String, String, String)> = database.connection.query_row(
        "SELECT s.worktree,json_extract(a.body,'$.status'),t.kind,s.id FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN sessions s ON s.id=t.session WHERE a.turn=?1",
        [turn.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().map_err(storage_error)?;
    let (worktree, state, kind, session) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn does not exist"))?;
    if kind != "task" {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "context compaction cannot execute commands",
        ));
    }
    if state != "running" {
        return Err(Fault::new(ErrorCode::Conflict, "turn is not running"));
    }
    if attachments.len() > 8
        || attachments
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != attachments.len()
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid command attachments",
        ));
    }
    let inputs = if attachments.is_empty() {
        None
    } else {
        Some(crate::files::attachments::Inputs {
            profile: database.profile.clone().ok_or_else(super::unavailable)?,
            files: attachments
                .iter()
                .map(|id| super::attachments::turns::resolve(&database.connection, *turn, *id))
                .collect::<Result<_, _>>()?,
        })
    };
    let launch = Launch {
        root: database.worktree_root(worktree.parse().map_err(storage_error)?)?,
        command: command.clone(),
        cwd: cwd.clone(),
        timeout_ms: *timeout_ms,
        inputs,
    };
    launch.validate()?;
    Ok(Prepared {
        launch,
        session: session.parse().map_err(storage_error)?,
        background: *background,
    })
}

pub(super) struct Worker {
    pub pending: usize,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
    agents: Arc<crate::agent::Controls>,
    environment: Arc<Environment>,
    capacity: Arc<Semaphore>,
    commands: Arc<process::background::Commands>,
}

impl Worker {
    pub fn new(
        runtime: Handle,
        completed: mpsc::Sender<Job>,
        closed: CancellationToken,
        agents: Arc<crate::agent::Controls>,
        commands: Arc<process::background::Commands>,
    ) -> Self {
        Self {
            pending: 0,
            commands,
            runtime,
            completed,
            closed,
            agents,
            environment: Arc::new(Environment::capture()),
            capacity: Arc::new(Semaphore::new(8)),
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        if self.pending >= CAPACITY || self.closed.is_cancelled() {
            return Err(Box::new(execution));
        }
        let Command::RunCommand { turn, .. } = execution.request.command else {
            unreachable!()
        };
        let Some(stop) = self.agents.begin_operation(turn) else {
            return Err(Box::new(execution));
        };
        self.pending += 1;
        let closed = self.closed.clone();
        let completed = self.completed.clone();
        let environment = self.environment.clone();
        let capacity = self.capacity.clone();
        let commands = self.commands.clone();
        self.runtime.spawn(async move {
            let operation = async {
                if execution.launch.background {
                    if stop.is_cancelled() || closed.is_cancelled() {
                        return Ok(Output::CommandResult(process::cancelled()));
                    }
                    return commands
                        .start(
                            execution.request.id,
                            execution.launch.session,
                            turn,
                            execution.launch.launch,
                            environment,
                            closed,
                        )
                        .map(Output::CommandStarted);
                }
                let permit = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(Output::CommandResult(process::cancelled())),
                    _ = closed.cancelled() => return Ok(Output::CommandResult(process::cancelled())),
                    permit = capacity.acquire_owned() => permit.map_err(|_| super::unavailable())?,
                };
                let _permit = permit;
                process::execute(execution.launch.launch, &environment, &stop, &closed)
                    .await
                    .map(Output::CommandResult)
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "command execution failed; inspect the target before retrying",
                    ))
                });
            let _ = completed
                .send(Job::Executed(Box::new(Completed {
                    caller: execution.caller,
                    request: execution.request,
                    result,
                    reply: execution.reply,
                })))
                .await;
        });
        Ok(())
    }
}

impl super::Ingress {
    pub(super) fn read_command(&self, request: Request) -> Result<sailry_link::Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let result = match request.command {
            Command::ListCommands { session } => Ok(Output::Commands(self.commands.list(session))),
            Command::ReadCommand { session, id } => {
                self.commands.read(session, id).map(Output::CommandOutput)
            }
            _ => unreachable!(),
        };
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(result);
        Ok(sailry_link::Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
