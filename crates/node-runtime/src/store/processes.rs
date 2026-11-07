//! Managed commands share durable admission; waiting never occupies the file mutation worker.
use super::{
    Job,
    database::{Database, storage_error},
    external::Completed,
};
use crate::process::{self, Environment, Launch};
use futures::FutureExt;

use sailry_link::{CancellationToken, Response};
use sailry_protocol::*;
use std::{panic::AssertUnwindSafe, sync::Arc};
use tokio::{
    runtime::Handle,
    sync::{mpsc, oneshot},
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

pub(super) async fn prepare(database: &Database, request: &Request) -> Result<Prepared, Fault> {
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
        ..
    } = &request.command
    else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "command execution expected",
        ));
    };
    if description
        .as_ref()
        .is_some_and(|description| description.trim().is_empty() || description.contains('\0'))
    {
        return Err(Fault::invalid("invalid command description"));
    }
    let row: Option<(String, String, String, String)> = {
        let mut statement = database.connection.prepare("SELECT r.worktree,json_extract(a.body,'$.status'),t.kind,t.session FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE a.turn=?1").await.map_err(storage_error)?;
        match statement.query_row([turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))(
                        &row,
                    );
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (worktree, state, kind, session) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn does not exist"))?;
    if kind != "task" {
        return Err(Fault::invalid("context compaction cannot execute commands"));
    }
    if state != "running" {
        return Err(Fault::new(ErrorCode::Conflict, "turn is not running"));
    }
    if attachments
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
            files: {
                let mut selected = Vec::new();
                for value in attachments.iter() {
                    selected.push(
                        async {
                            let id: &sailry_protocol::AttachmentId = value;
                            super::attachments::turns::resolve(&database.connection, *turn, *id)
                                .await
                        }
                        .await?,
                    );
                }
                Ok::<_, Fault>(selected)
            }?,
        })
    };
    let worktree = worktree.parse().map_err(storage_error)?;
    let profile = database.profile.clone().ok_or_else(super::unavailable)?;
    let launch = Launch {
        pty: *pty,
        root: database.worktree_root(worktree).await?,
        profile: Some(profile.clone()),
        command: command.clone(),
        access: *access,
        cwd: cwd.clone(),
        timeout_ms: *timeout_ms,
        inputs,
        scratch: None,
        python: None,
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
    commands: Arc<process::background::Commands>,
    code: Arc<crate::code_index::Service>,
    plugins: crate::plugins::Host,
    git: crate::git::Git,
    ingress: std::sync::Weak<super::Ingress>,
}

/// A foreground command running this long moves to the background once the
/// turn has pending guidance, so the guidance reaches the next model boundary.
const GUIDANCE_PROMOTION: std::time::Duration = std::time::Duration::from_secs(30);

/// Resolves when the turn has guidance waiting for its next boundary.
async fn guidance(ingress: &std::sync::Weak<super::Ingress>, turn: TurnId) {
    let Some(notice) = ingress
        .upgrade()
        .and_then(|ingress| ingress.agents.guidance_notice(turn))
    else {
        return std::future::pending().await;
    };
    loop {
        let notified = notice.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let pending = match ingress.upgrade() {
            Some(ingress) => ingress.pending_guidance(turn).await.unwrap_or(false),
            None => false,
        };
        if pending {
            return;
        }
        notified.await;
    }
}

/// Runs a foreground command. Without pending guidance it behaves exactly like
/// a direct execution; once `promotion` resolves (guidance after
/// `GUIDANCE_PROMOTION`) the process continues as a background command
/// instead of being stopped.
#[allow(clippy::too_many_arguments)]
async fn foreground(
    launch: Launch,
    id: RequestId,
    environment: Arc<Environment>,
    commands: Arc<process::background::Commands>,
    retained: Arc<process::output::Retained>,
    stop: CancellationToken,
    closed: CancellationToken,
    promotion: impl std::future::Future<Output = ()>,
    gate: oneshot::Receiver<()>,
) -> Result<Output, Fault> {
    // The process owns its own token so promotion can detach it from the turn.
    let own = CancellationToken::new();
    let (done, mut outcome) = oneshot::channel();
    let (promoted, settle) = oneshot::channel::<()>();
    let task = {
        let own = own.clone();
        let closed = closed.clone();
        let commands = commands.clone();
        tokio::spawn(async move {
            let result = AssertUnwindSafe(process::execute_retained(
                launch,
                &environment,
                &own,
                &closed,
                retained,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "command execution failed; inspect the target before retrying",
                ))
            });
            match done.send(result) {
                Ok(()) => {}
                Err(result) => {
                    // Promoted: the tool already returned. Report completion only
                    // after the receipt that registered the background record.
                    let _ = settle.await;
                    let _ = gate.await;
                    commands.complete(id, result.as_ref(), &closed);
                }
            }
        })
    };
    let forward = async {
        stop.cancelled().await;
        own.cancel();
        std::future::pending::<()>().await
    };
    tokio::select! {
        biased;
        result = &mut outcome => return result.unwrap_or_else(|_| Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "command execution failed; inspect the target before retrying",
        ))).map(Output::CommandResult),
        _ = forward => unreachable!(),
        _ = promotion => {}
    }
    // Close the race with a completion that arrived while promoting.
    outcome.close();
    if let Ok(result) = outcome.try_recv() {
        return result.map(Output::CommandResult);
    }
    match commands.promote(id, own, task) {
        Some(info) => {
            let _ = promoted.send(());
            Ok(Output::CommandStarted(info))
        }
        None => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "command state changed while moving it to the background; inspect it before retrying",
        )),
    }
}

impl Worker {
    pub fn new(
        runtime: Handle,
        completed: mpsc::Sender<Job>,
        ingress: &Arc<super::Ingress>,
    ) -> Self {
        Self {
            pending: 0,
            code: ingress.code_index.clone(),
            plugins: ingress.plugins.clone(),
            git: ingress.git.clone(),
            commands: ingress.commands.clone(),
            runtime,
            completed,
            closed: ingress.closed.clone(),
            agents: ingress.agents.clone(),
            environment: ingress.environment.clone(),
            ingress: Arc::downgrade(ingress),
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        if self.closed.is_cancelled() {
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
        let commands = self.commands.clone();
        let code = self.code.clone();
        let agents = self.agents.clone();
        let plugins = self.plugins.clone();
        let git = self.git.clone();
        let ingress = self.ingress.clone();
        let (open, gate) = oneshot::channel();
        self.runtime.spawn(async move {
            let mut execution = execution;
            let root = execution.launch.launch.root.clone();
            let operation = async {
                execution.launch.launch.scratch = Some(agents.scratch(
                    execution.launch.turn,
                    &execution.launch.profile,
                    execution.launch.worktree,
                    execution.launch.session,
                )?);
                let preparing = stop.child_token();
                let stopping = preparing.clone();
                let closing = closed.clone();
                let watch = tokio::spawn(async move {
                    closing.cancelled().await;
                    stopping.cancel();
                });
                let prepared = plugins
                    .command_python(
                        &environment,
                        execution.launch.session,
                        &agents.python(turn),
                        &preparing,
                    )
                    .await;
                watch.abort();
                execution.launch.launch.python = match prepared {
                    Ok(python) => python,
                    Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
                    // Managed Python is optional for ordinary shell commands.
                    // A command explicitly using it still reports its own failure.
                    Err(_) => None,
                };
                if execution.launch.background {
                    if stop.is_cancelled() || closed.is_cancelled() {
                        return Ok(Output::CommandResult(process::cancelled()));
                    }
                    let info = commands.start(
                        execution.request.id,
                        execution.launch.session,
                        turn,
                        execution.launch.launch,
                        environment,
                        closed,
                    )?;
                    commands.watch(
                        info.id,
                        execution.launch.worktree,
                        code,
                        root.clone(),
                        git.clone(),
                    );
                    return Ok(Output::CommandStarted(info));
                }
                let retained = commands.retain(
                    execution.request.id,
                    execution.launch.session,
                    turn,
                    &execution.launch.launch,
                );
                let output = foreground(
                    execution.launch.launch,
                    execution.request.id,
                    environment,
                    commands.clone(),
                    retained,
                    stop,
                    closed,
                    async {
                        tokio::time::sleep(GUIDANCE_PROMOTION).await;
                        guidance(&ingress, turn).await;
                    },
                    gate,
                )
                .await;
                if let Ok(Output::CommandStarted(info)) = &output {
                    commands.watch(
                        info.id,
                        execution.launch.worktree,
                        code,
                        root.clone(),
                        git.clone(),
                    );
                }
                output
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
            if !execution.launch.background && !matches!(result, Ok(Output::CommandStarted(_))) {
                git.invalidate(&root);
                match &result {
                    Ok(Output::CommandResult(completion)) => {
                        commands.finish(execution.request.id, Ok(completion))
                    }
                    Err(error) => commands.finish(execution.request.id, Err(error)),
                    _ => {}
                }
            }
            let promoted =
                matches!(result, Ok(Output::CommandStarted(_))) && !execution.launch.background;
            let reply = if promoted {
                // Storage replies only after committing the receipt that
                // registers the background record; completion waits for it.
                let (reply, committed) = oneshot::channel();
                let caller = execution.reply;
                tokio::spawn(async move {
                    if let Ok(response) = committed.await {
                        let _ = caller.send(response);
                    }
                    let _ = open.send(());
                });
                reply
            } else {
                drop(open);
                execution.reply
            };
            let _ = completed
                .send(Job::Executed(Box::new(Completed {
                    caller: execution.caller,
                    request: execution.request,
                    result,
                    reply,
                })))
                .await;
        });
        Ok(())
    }
}

impl super::Ingress {
    pub(super) async fn read_command(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<sailry_link::Admission, Fault> {
        if matches!(
            &request.command,
            Command::ReadCommand { wait_ms, .. } | Command::ReadCommandPage { wait_ms, .. }
                if *wait_ms > 0
        ) {
            let commands = self.commands.clone();
            let closed = self.closed.clone();
            let waiting = match &request.command {
                Command::ReadCommand {
                    session,
                    id,
                    wait_ms,
                    until,
                }
                | Command::ReadCommandPage {
                    session,
                    id,
                    wait_ms,
                    until,
                    ..
                } => commands.wait(*session, *id, *wait_ms, until.clone(), closed),
                _ => unreachable!(),
            };
            let (mut reply, completion) = oneshot::channel();
            tokio::spawn(async move {
                let work = async {
                    waiting.await?;
                    match request.command {
                        Command::ReadCommand { session, id, .. } => {
                            commands.read(session, id).map(Output::CommandOutput)
                        }
                        Command::ReadCommandPage {
                            session,
                            id,
                            stream,
                            offset,
                            limit,
                            ..
                        } => commands
                            .page(session, id, stream, offset, limit)
                            .map(Output::CommandOutputPage),
                        _ => unreachable!(),
                    }
                };
                tokio::select! {
                    _ = reply.closed() => {},
                    result = work => { let _ = reply.send(result); },
                }
            });
            return Ok(sailry_link::Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            });
        }
        let result = match request.command {
            Command::ListCommands { session } => Ok(Output::Commands(self.commands.list(session))),
            Command::ReadCommand { session, id, .. } => {
                self.commands.read(session, id).map(Output::CommandOutput)
            }
            Command::ReadCommandPage {
                session,
                id,
                stream,
                offset,
                limit,
                ..
            } => self
                .commands
                .page(session, id, stream, offset, limit)
                .map(Output::CommandOutputPage),
            Command::ReadToolOutput {
                turn,
                output,
                stream,
                offset,
                limit,
            } => {
                let storage = self.storage.get().ok_or_else(super::unavailable)?;
                let reference = output.clone();
                let resolved = storage
                    .read(move |db| {
                        Box::pin(async move {
                            super::agent::history::output::resolve(db, caller, turn, &reference)
                                .await
                        })
                    })
                    .await?;
                match stream {
                    Some(stream) => {
                        let id = crate::process::output::command_id(&resolved.value)
                            .and_then(|id| id.parse().ok())
                            .ok_or_else(|| {
                                Fault::invalid("tool result has no retained command stream")
                            })?;
                        self.commands
                            .page(resolved.session, id, stream, offset, limit)
                            .map(Output::CommandOutputPage)
                    }
                    None => super::agent::history::output::page(resolved, output, offset, limit)
                        .map(Output::ToolOutput),
                }
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
