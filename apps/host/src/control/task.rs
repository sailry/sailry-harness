//! Noninteractive task control observes the existing Node's canonical admission and history.
mod observe;
mod options;
#[cfg(test)]
mod tests;

use super::Error;
use sailry_client::Client;
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{conversation, *};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, sync::Arc};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

pub(super) const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Task {
    Submit {
        version: u8,
        request: RequestId,
        target: Target,
        message: conversation::Input,
        title: String,
        wait: bool,
        timeout: u64,
    },
    Wait {
        version: u8,
        session: SessionId,
        turn: TurnId,
        timeout: u64,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Session {
        session: SessionId,
        expected_revision: Option<u64>,
    },
    Worktree {
        worktree: WorktreeId,
        config: Option<Box<SessionConfig>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Admitted,
    Observing,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Continued,
    AwaitingApproval,
    AwaitingInput,
    Paused,
    TimedOut,
    Uncertain,
    ObservationCancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Result {
    version: u8,
    node: Option<NodeId>,
    request: Option<RequestId>,
    session: Option<SessionId>,
    turn: Option<TurnId>,
    revision: Option<u64>,
    state: State,
    text: String,
    error: Option<Fault>,
    approvals: Vec<conversation::Approval>,
    questions: Vec<conversation::question::Question>,
}

impl Result {
    fn new(node: Option<NodeId>) -> Self {
        Self {
            version: 1,
            node,
            request: None,
            session: None,
            turn: None,
            revision: None,
            state: State::Admitted,
            text: String::new(),
            error: None,
            approvals: Vec::new(),
            questions: Vec::new(),
        }
    }

    fn failed(mut self, error: Fault) -> Self {
        self.state = if error.code == ErrorCode::OutcomeUnknown {
            State::Uncertain
        } else {
            State::Failed
        };
        self.error = Some(error);
        self
    }

    fn exit_code(&self) -> u8 {
        match self.state {
            State::Admitted | State::Observing | State::Completed => 0,
            State::AwaitingApproval | State::AwaitingInput | State::Paused => 3,
            State::TimedOut => 124,
            State::ObservationCancelled => 130,
            State::Uncertain => 4,
            _ => 1,
        }
    }
}

impl Task {
    fn result(&self) -> Result {
        let mut result = Result::new(None);
        match self {
            Self::Submit {
                request, target, ..
            } => {
                result.request = Some(*request);
                if let Target::Session {
                    session,
                    expected_revision,
                } = target
                {
                    result.session = Some(*session);
                    result.revision = *expected_revision;
                }
            }
            Self::Wait { session, turn, .. } => {
                result.session = Some(*session);
                result.turn = Some(*turn);
                result.state = State::Observing;
            }
        }
        result
    }
}

fn parse(line: &str) -> std::result::Result<Task, Fault> {
    if line.len() > MAX_REQUEST || !line.ends_with('\n') {
        return Err(Fault::invalid("invalid Host task request"));
    }
    let task: Task =
        serde_json::from_str(line).map_err(|_| Fault::invalid("invalid Host task request"))?;
    let (version, timeout) = match &task {
        Task::Submit {
            version, timeout, ..
        }
        | Task::Wait {
            version, timeout, ..
        } => (*version, *timeout),
    };
    if version != 1 || !(1..=86400).contains(&timeout) {
        return Err(Fault::invalid("unsupported Host task request"));
    }
    Ok(task)
}

pub(super) async fn serve<R, W>(
    line: &str,
    mut reader: R,
    mut writer: W,
    transport: Arc<dyn Transport>,
    stop: CancellationToken,
) -> std::result::Result<(), Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let client = Arc::new(Client::new(transport));
    let mut result = Result::new(Some(client.target()));
    let task = match parse(line) {
        Ok(task) => task,
        Err(error) => return write(&mut writer, &result.failed(error)).await,
    };
    let (session, turn, timeout) = match task {
        Task::Submit {
            request,
            target,
            message,
            title,
            wait,
            timeout,
            ..
        } => {
            result.request = Some(request);
            let command = match target {
                Target::Session {
                    session,
                    expected_revision,
                } => {
                    result.session = Some(session);
                    result.revision = expected_revision;
                    Command::SubmitTask {
                        session,
                        expected_revision,
                        message,
                    }
                }
                Target::Worktree { worktree, config } => {
                    let project = match client
                        .execute(client.prepare(Command::ReadWorktreeCatalog { worktree }))
                        .await
                    {
                        Ok(Output::ProjectCatalog(catalog)) => catalog
                            .worktrees
                            .iter()
                            .find(|entry| entry.id == worktree)
                            .and_then(|entry| entry.project),
                        Ok(_) => {
                            return write(
                                &mut writer,
                                &result.failed(Fault::new(
                                    ErrorCode::Internal,
                                    "worktree catalog expected",
                                )),
                            )
                            .await;
                        }
                        Err(error) => return write(&mut writer, &result.failed(error)).await,
                    };
                    if project.is_none() {
                        return write(
                            &mut writer,
                            &result.failed(Fault::invalid(
                                "new task requires a registered project worktree",
                            )),
                        )
                        .await;
                    }
                    Command::StartSession(conversation::Start {
                        project,
                        worktree: Some(worktree),
                        config: config.map(|config| *config),
                        title,
                        message,
                    })
                }
            };
            let mut prepared = client.prepare(command);
            prepared.id = request;
            let turn = match client.execute(prepared).await {
                Ok(Output::QueuedTurn(turn)) => turn,
                Ok(_) => {
                    return write(
                        &mut writer,
                        &result.failed(Fault::new(ErrorCode::Internal, "turn admission expected")),
                    )
                    .await;
                }
                Err(error) => return write(&mut writer, &result.failed(error)).await,
            };
            result.session = Some(turn.session);
            result.turn = Some(turn.id);
            result.revision = Some(turn.revision);
            // This follows durable business admission, not only a transport receipt.
            write(&mut writer, &result).await?;
            if !wait {
                return Ok(());
            }
            (turn.session, turn.id, timeout)
        }
        Task::Wait {
            session,
            turn,
            timeout,
            ..
        } => {
            result.session = Some(session);
            result.turn = Some(turn);
            result.state = State::Observing;
            write(&mut writer, &result).await?;
            (session, turn, timeout)
        }
    };
    let mut disconnected = [0];
    let observation = observe::wait(client, session, turn, timeout, &stop);
    tokio::pin!(observation);
    let outcome = tokio::select! {
        outcome = &mut observation => outcome,
        _ = reader.read(&mut disconnected) => return Ok(()),
    };
    // Disconnecting a management observer never cancels the admitted Agent task.
    match outcome {
        Ok(observed) => {
            result.state = observed.state;
            result.revision = observed.revision.or(result.revision);
            result.text = observed.text;
            result.error = observed.error;
            result.approvals = observed.approvals;
            result.questions = observed.questions;
        }
        Err(error) => {
            result.state = State::Uncertain;
            result.error = Some(error);
        }
    }
    write(&mut writer, &result).await
}

async fn write(
    writer: &mut (impl AsyncWrite + Unpin),
    result: &Result,
) -> std::result::Result<(), Error> {
    let bytes = serde_json::to_vec(result)?;
    if bytes.len() > MAX_RESPONSE {
        return Err(
            "Host task response exceeds its limit; read canonical history through Client".into(),
        );
    }
    writer.write_all(&bytes).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

pub(crate) async fn run(
    arguments: impl Iterator<Item = OsString>,
) -> std::result::Result<u8, Box<dyn std::error::Error>> {
    let Some(options) = options::parse(arguments)? else {
        return Ok(0);
    };
    let bytes = serde_json::to_vec(&options.task)?;
    if bytes.len() + 1 > MAX_REQUEST {
        return Err("Host task request exceeds its limit".into());
    }
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut known = options.task.result();
    let stream = match tokio::net::UnixStream::connect(options.profile.join("control.sock")).await {
        Ok(stream) => stream,
        Err(_) => {
            return print(&known.failed(Fault::new(
                ErrorCode::Unavailable,
                "Host is not running; use sailry start",
            )));
        }
    };
    let (reader, mut writer) = stream.into_split();
    if writer.write_all(&bytes).await.is_err() || writer.write_all(b"\n").await.is_err() {
        return print(&uncertain(
            known,
            "Host task submission is unconfirmed; retain the same request identifier",
        ));
    }
    let mut reader = BufReader::new(reader);
    loop {
        let mut line = String::new();
        let count = tokio::select! {
            _ = interrupt.recv() => {
                known.state = State::ObservationCancelled;
                return print(&known);
            },
            result = async {
                let mut limited = (&mut reader).take((MAX_RESPONSE + 1) as u64);
                limited.read_line(&mut line).await
            } => match result {
                Ok(count) => count,
                Err(_) => return print(&uncertain(known, "Host task observation disconnected; retain the same request or turn identifier")),
            },
        };
        if count == 0 || count > MAX_RESPONSE || !line.ends_with('\n') {
            return print(&uncertain(
                known,
                "Host task observation disconnected; retain the same request or turn identifier",
            ));
        }
        let result: Result = match serde_json::from_str(&line) {
            Ok(result) => result,
            Err(_) => {
                return print(&uncertain(
                    known,
                    "Host task observation returned an invalid response",
                ));
            }
        };
        if !matches_response(&known, &result) {
            return print(&uncertain(
                known,
                "Host task observation returned mismatched identifiers",
            ));
        }
        if matches!(result.state, State::Admitted | State::Observing) && options.wait {
            known = result;
            continue;
        }
        return print(&result);
    }
}

fn matches_response(known: &Result, incoming: &Result) -> bool {
    incoming.version == 1
        && incoming.node.is_some()
        && known.node.is_none_or(|node| incoming.node == Some(node))
        && incoming.request == known.request
        && known
            .session
            .is_none_or(|session| incoming.session == Some(session))
        && known.turn.is_none_or(|turn| incoming.turn == Some(turn))
        && known
            .revision
            .is_none_or(|revision| incoming.revision == Some(revision))
        && (!matches!(incoming.state, State::Admitted | State::Observing)
            || incoming.session.is_some() && incoming.turn.is_some())
}

fn uncertain(mut result: Result, message: &str) -> Result {
    result.state = State::Uncertain;
    result.error = Some(Fault::new(ErrorCode::OutcomeUnknown, message));
    result
}

fn print(result: &Result) -> std::result::Result<u8, Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(result)?);
    Ok(result.exit_code())
}
