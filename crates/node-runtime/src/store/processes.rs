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
    profile: std::path::PathBuf,
    worktree: WorktreeId,
    session: SessionId,
    turn: TurnId,
    background: bool,
}

pub(super) fn prepare(database: &Database, request: &Request) -> Result<Prepared, Fault> {
    let Command::RunCommand {
        turn,
        command,
        access,
        description,
        cwd,
        timeout_ms,
        background,
        pty,
        attachments,
    } = &request.command
    else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "command execution expected",
        ));
    };
    if description.as_ref().is_some_and(|description| {
        description.trim().is_empty()
            || description.chars().count() > 2000
            || description.contains('\0')
    }) {
        return Err(Fault::invalid("invalid command description"));
    }
    let row: Option<(String, String, String, String)> = database.connection.query_row(
        "SELECT r.worktree,json_extract(a.body,'$.status'),t.kind,t.session FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE a.turn=?1",
        [turn.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().map_err(storage_error)?;
    let (worktree, state, kind, session) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn does not exist"))?;
    if kind != "task" {
        return Err(Fault::invalid("context compaction cannot execute commands"));
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
        return Err(Fault::invalid("invalid command attachments"));
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
    let worktree = worktree.parse().map_err(storage_error)?;
    let profile = database.profile.clone().ok_or_else(super::unavailable)?;
    let launch = Launch {
        pty: *pty,
        root: database.worktree_root(worktree)?,
        profile: Some(profile.clone()),
        command: command.clone(),
        access: *access,
        cwd: cwd.clone(),
        timeout_ms: *timeout_ms,
        inputs,
        scratch: None,
    };
    launch.validate()?;
    if *pty && !background {
        return Err(Fault::invalid("PTY commands must run in the background"));
    }
    Ok(Prepared {
        launch,
        profile,
        worktree,
        session: session.parse().map_err(storage_error)?,
        turn: *turn,
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
    code: Arc<crate::code::Pool>,
}

impl Worker {
    pub fn new(
        runtime: Handle,
        completed: mpsc::Sender<Job>,
        closed: CancellationToken,
        agents: Arc<crate::agent::Controls>,
        commands: Arc<process::background::Commands>,
        environment: Arc<Environment>,
        code: Arc<crate::code::Pool>,
    ) -> Self {
        Self {
            pending: 0,
            code,
            commands,
            runtime,
            completed,
            closed,
            agents,
            environment,
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
        let code = self.code.clone();
        let agents = self.agents.clone();
        self.runtime.spawn(async move {
            let mut execution = execution;
            let operation = async {
                execution.launch.launch.scratch = Some(agents.scratch(
                    execution.launch.turn,
                    &execution.launch.profile,
                    execution.launch.worktree,
                    execution.launch.session,
                )?);
                if execution.launch.background {
                    if stop.is_cancelled() || closed.is_cancelled() {
                        return Ok(Output::CommandResult(process::cancelled()));
                    }
                    let info=commands
                        .start(
                            execution.request.id,
                            execution.launch.session,
                            turn,
                            execution.launch.launch,
                            environment,
                            closed,
                        )?;
                    commands.watch(info.id,execution.launch.worktree,code);
                    return Ok(Output::CommandStarted(info));
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

#[cfg(test)]
mod tests;
