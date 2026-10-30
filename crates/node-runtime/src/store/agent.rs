//! GDK uses the existing Node storage worker and canonical conversation owner.
pub(super) mod activity;
pub(super) mod approvals;
pub(super) mod assets;
pub(in crate::store) mod background;
pub(crate) mod blobs;
mod calls;
pub(super) mod checkpoints;
mod children;
pub(crate) mod compaction;
pub(crate) mod connections;
pub(in crate::store) mod continuation;
pub(in crate::store) mod delegation;
pub(crate) mod elicitation;
mod feed;
pub(in crate::store) mod history;
mod images;
mod permissions;
pub(in crate::store) mod plugins;
pub(in crate::store) use plugins::package as plugin_package;
mod presentation;
pub(super) mod providers;
pub(super) mod questions;
pub(super) mod queue;
pub(crate) mod references;
pub(crate) mod review;
pub(in crate::store) mod runs;
mod search;
mod sessions;
pub(super) mod statistics;
mod telemetry;
#[cfg(test)]
mod tests;
mod worktrees;

use super::{
    Database, Ingress, Job,
    database::{encode, storage_error},
};
use sailry_protocol::{conversation::*, *};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};
use turso::{Connection, params};

pub(crate) use approvals::Binding as ApprovalBinding;
pub(super) use delegation::read as delegation;
pub(in crate::store) use feed::run as publish_run;
pub(super) use feed::{Feeds, command as publish_command, package as invalidate_package};
pub(super) use history::pages::{read, turn as read_turn};
pub(super) use history::{
    rebuild_index as rebuild_event_index, rebuild_session as rebuild_session_state,
};
pub(super) use providers::{list as providers, put as put_provider, remove as remove_provider};
pub(super) use runs::visible as visible_run;
pub(super) use runs::{admit, recover, start, stop, stopping};
pub(super) use search::search;
pub(crate) use sessions::{Query as SessionQuery, Session as RuntimeSession, Sessions};

pub(crate) const SESSION_TITLE: &str = "sailry.session_title";
pub(crate) const LOADED_TOOLS: &str = "sailry.context.loaded_tools";

// Job boxes the entire operation; boxing its event again adds no queue saving.
#[allow(clippy::large_enum_variant)]
pub(super) enum Operation {
    ClaimHook {
        turn: TurnId,
        phase: hooks::Phase,
        ordinal: u64,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    FinishHook {
        turn: TurnId,
        phase: hooks::Phase,
        ordinal: u64,
        result: Result<process::Outcome, Fault>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Worktrees {
        turn: TurnId,
        reply: oneshot::Sender<Result<Vec<Worktree>, Fault>>,
    },
    ReadHistory {
        source: references::history::Source,
        query: references::history::Query,
        reply: oneshot::Sender<Result<serde_json::Value, Fault>>,
    },
    Connection {
        turn: TurnId,
        package: plugin::Reference,
        target: Option<sailry_protocol::connection::Resource>,
        action: connections::Action,
        captured: Vec<sailry_protocol::connection::Resource>,
        reply: oneshot::Sender<Result<Command, Fault>>,
    },
    ConnectionStatus {
        turn: TurnId,
        package: plugin::Reference,
        targets: Vec<sailry_protocol::connection::Resource>,
        captured: Vec<sailry_protocol::connection::Resource>,
        reply: oneshot::Sender<
            Result<std::collections::BTreeMap<sailry_protocol::connection::Resource, bool>, Fault>,
        >,
    },
    ConnectionNames {
        turn: TurnId,
        entries: Vec<(plugin::Reference, sailry_protocol::connection::Resource)>,
        captured: Vec<sailry_protocol::connection::Resource>,
        reply: oneshot::Sender<Result<Vec<Option<String>>, Fault>>,
    },
    ConnectionTrust {
        turn: TurnId,
        package: plugin::Reference,
        inspection: connections::Inspection,
        captured: Vec<sailry_protocol::connection::Resource>,
        reply: oneshot::Sender<Result<ssh::Trust, Fault>>,
    },
    Attachment {
        turn: TurnId,
        id: AttachmentId,
        reply: oneshot::Sender<Result<(attachment::Attachment, bool), Fault>>,
    },
    Image {
        session: SessionId,
        image: Image,
        reply: oneshot::Sender<Result<Vec<u8>, Fault>>,
    },
    Delegate {
        turn: TurnId,
        request: crate::agent::canonical::ToolIdentity,
        arguments: serde_json::Value,
        message: Option<Input>,
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Invocation, Fault>>,
    },
    ClaimChild {
        turn: TurnId,
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Status, Fault>>,
    },
    FinishQueuedChild {
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
        reply: oneshot::Sender<Result<Status, Fault>>,
    },
    CommandFinished(crate::process::background::Finished),
    ConsumeCommand {
        parent: TurnId,
        snapshot: sailry_protocol::process::Snapshot,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    CollectChild {
        parent: TurnId,
        child: SessionId,
        turn: TurnId,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    ChildState {
        parent: TurnId,
        child: SessionId,
        exact: Option<TurnId>,
        reply: oneshot::Sender<Result<delegation::State, Fault>>,
    },
    PendingGuidance {
        recipient: TurnId,
        reply: oneshot::Sender<Result<bool, Fault>>,
    },
    Checkpoint {
        turn: TurnId,
        request: Request,
        before: Option<String>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Ask {
        turn: TurnId,
        request: crate::agent::canonical::ToolIdentity,
        reply: oneshot::Sender<Result<question::Response, Fault>>,
    },
    ExpireQuestion {
        turn: TurnId,
        call: String,
        stop: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<question::Question, Fault>>,
    },
    CancelInput {
        turn: TurnId,
        call: String,
    },
    Confirm {
        turn: TurnId,
        owner: RequestId,
        deadline: std::time::Instant,
        request: crate::agent::canonical::ToolIdentity,
        binding: Option<ApprovalBinding>,
        ssh: Option<sailry_protocol::ssh::Trust>,
        prepared: Option<Command>,
        reply: oneshot::Sender<Result<Decision, Fault>>,
    },
    CancelApproval {
        turn: TurnId,
        call: String,
        owner: RequestId,
    },
    #[cfg(any(test, feature = "test-support"))]
    ApprovalCommitFixture {
        gate: approvals::fixture::Commit,
        reply: oneshot::Sender<()>,
    },
    Authorize {
        turn: TurnId,
        request: crate::agent::canonical::ToolIdentity,
        prepared: Option<Command>,
        reply: oneshot::Sender<Result<RequestId, Fault>>,
    },
    ConnectionAccess {
        turn: TurnId,
        request: crate::agent::canonical::ToolIdentity,
        prepared: Command,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    CommandAccess {
        turn: TurnId,
        request: crate::agent::canonical::ToolIdentity,
        reply: oneshot::Sender<Result<process::Access, Fault>>,
    },
    Subscribe {
        session: SessionId,
        closed: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Box<dyn sailry_link::Subscription>, Fault>>,
    },
    Get {
        turn: TurnId,
        request: sessions::Query,
        reply: oneshot::Sender<Result<sessions::Session, Fault>>,
    },
    Effects {
        session: SessionId,
        turn: TurnId,
        effects: Vec<crate::agent::engine::Effect>,
        presentation: crate::agent::canonical::Presentation,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Append {
        session: SessionId,
        turn: TurnId,
        event: crate::agent::canonical::Record,
        context: Option<(TurnId, crate::agent::canonical::Record)>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Claim {
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Option<Invocation>, Fault>>,
    },
    ClaimGuidance {
        recipient: TurnId,
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Option<Invocation>, Fault>>,
    },
    TakeGuidance {
        recipient: TurnId,
        reply: oneshot::Sender<Result<Option<Vec<QueuedMessage>>, Fault>>,
    },
    RetainGuidance {
        recipient: TurnId,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Finish {
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
        owner: Option<sailry_link::CancellationToken>,
        review: Option<crate::agent::canonical::Record>,
        reply: oneshot::Sender<Result<Status, Fault>>,
    },
}

pub(crate) struct Invocation {
    pub instructions: Vec<super::captures::Instruction>,
    pub hooks: hooks::Settings,
    pub automatic: bool,
    pub notice: bool,
    pub notice_input: Option<String>,
    pub notices: Vec<sailry_protocol::conversation::background::Notice>,
    pub title: Option<String>,
    pub connections: connections::Catalog,
    pub turn: QueuedTurn,
    pub caller: NodeId,
    pub worktree: WorktreeId,
    pub project: Option<sailry_protocol::ProjectId>,
    pub provider: Option<Provider>,
    pub media: super::media::Models,
    pub message: Input,
    pub input_turn: Option<TurnId>,
    pub followups: Vec<sailry_protocol::conversation::QueuedMessage>,
    pub child: Option<Child>,
    pub plugins: Vec<plugin::Info>,
    pub plugin_settings:
        std::collections::BTreeMap<String, Result<crate::plugins::settings::Resolved, Fault>>,
}

pub(crate) struct Child {
    pub role: Option<role::Profile>,
}

pub(crate) struct ContextUsage {
    #[cfg(test)]
    pub observed: bool,
    pub tokens: u64,
    pub pending: Vec<crate::agent::canonical::Record>,
}

impl Ingress {
    pub(super) async fn tool_image(
        &self,
        session: SessionId,
        image: Image,
    ) -> Result<Vec<u8>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Image {
            session,
            image,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn attachment(
        &self,
        turn: TurnId,
        id: AttachmentId,
    ) -> Result<attachment::Attachment, Fault> {
        self.attachment_source(turn, id)
            .await
            .map(|(attachment, _)| attachment)
    }

    pub(crate) async fn attachment_source(
        &self,
        turn: TurnId,
        id: AttachmentId,
    ) -> Result<(attachment::Attachment, bool), Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Attachment {
            turn,
            id,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn attachment_bytes(
        &self,
        attachment: attachment::Attachment,
        stop: sailry_link::CancellationToken,
    ) -> Result<Vec<u8>, Fault> {
        self.files
            .attachment(self.profile.clone(), attachment, stop)
            .await
    }

    pub(super) async fn conversation(
        &self,
        session: SessionId,
    ) -> Result<Box<dyn sailry_link::Subscription>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Subscribe {
            session,
            closed: self.closed.clone(),
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn partial(
        &self,
        session: SessionId,
        turn: TurnId,
        message: goose_provider_types::conversation::message::Message,
    ) -> Result<(), Fault> {
        if self.closed.is_cancelled() {
            return Err(super::unavailable());
        }
        let id = message
            .id
            .ok_or_else(|| Fault::invalid("streaming message identity is missing"))?;
        let parts = message
            .content
            .into_iter()
            .filter_map(|content| match content {
                goose_provider_types::conversation::message::MessageContent::Text(text) => {
                    Some(Part::Text(text.text))
                }
                goose_provider_types::conversation::message::MessageContent::Thinking(thinking) => {
                    Some(Part::Thinking(thinking.thinking))
                }
                _ => None,
            })
            .collect();
        self.agents.stream(turn, session, || {
            self.feeds.delta(
                self.node,
                session,
                Draft {
                    id,
                    turn,
                    author: "sailry".into(),
                    branch: String::new(),
                    parts,
                },
            )
        })
    }
    pub(crate) fn sessions(self: &Arc<Self>, turn: TurnId) -> Arc<Sessions> {
        Arc::new(Sessions::new(self.clone(), turn))
    }

    pub(crate) async fn claim_run(
        &self,
        token: sailry_link::CancellationToken,
    ) -> Result<Option<Invocation>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Claim { token, reply })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    #[cfg(test)]
    pub(crate) async fn finish_run(
        &self,
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
        owner: Option<sailry_link::CancellationToken>,
    ) -> Result<Status, Fault> {
        self.finish_run_with(turn, status, error, owner, None).await
    }

    pub(crate) async fn finish_run_with(
        &self,
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
        owner: Option<sailry_link::CancellationToken>,
        review: Option<crate::agent::canonical::Record>,
    ) -> Result<Status, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Finish {
            turn,
            status,
            error,
            owner,
            review,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn take_guidance(
        &self,
        recipient: TurnId,
    ) -> Result<Option<Vec<QueuedMessage>>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::TakeGuidance {
            recipient,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn claim_guidance(
        &self,
        recipient: TurnId,
        token: sailry_link::CancellationToken,
    ) -> Result<Option<Invocation>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ClaimGuidance {
            recipient,
            token,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn retain_guidance(&self, recipient: TurnId) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::RetainGuidance {
            recipient,
            reply,
        })))
        .await
        .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }
}

pub(super) fn readonly(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Worktrees { .. }
            | Operation::ReadHistory { .. }
            | Operation::Connection { .. }
            | Operation::ConnectionStatus { .. }
            | Operation::ConnectionNames { .. }
            | Operation::ConnectionTrust { .. }
            | Operation::Attachment { .. }
            | Operation::Image { .. }
            | Operation::ChildState { .. }
            | Operation::PendingGuidance { .. }
            | Operation::ConnectionAccess { .. }
            | Operation::CommandAccess { .. }
            | Operation::Subscribe { .. }
            | Operation::Get { .. }
    )
}

pub(super) fn execute<'a>(
    database: &'a mut Database,
    operation: Operation,
    events: &'a broadcast::Sender<EventEnvelope>,
    controls: &'a crate::agent::Controls,
) -> sailry_link::Pending<'a, ()> {
    // Select the handler before polling its async work.
    match operation {
        Operation::ClaimHook {
            turn,
            phase,
            ordinal,
            reply,
        } => Box::pin(async move {
            let _ =
                reply.send(super::hooks::claim(&database.connection, turn, phase, ordinal).await);
        }),
        Operation::FinishHook {
            turn,
            phase,
            ordinal,
            result,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                super::hooks::finish(&database.connection, turn, phase, ordinal, &result).await,
            );
        }),
        Operation::Worktrees { turn, reply } => Box::pin(async move {
            let _ = reply.send(worktrees::list(database, turn).await);
        }),
        Operation::ReadHistory {
            source,
            query,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(references::history::execute(database, source, query).await);
        }),
        Operation::Connection {
            turn,
            package,
            target,
            action,
            captured,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                connections::command(
                    &database.connection,
                    turn,
                    &package,
                    target,
                    action,
                    &captured,
                )
                .await,
            );
        }),
        Operation::ConnectionStatus {
            turn,
            package,
            targets,
            captured,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                connections::status(&database.connection, turn, &package, &targets, &captured)
                    .await,
            );
        }),
        Operation::ConnectionNames {
            turn,
            entries,
            captured,
            reply,
        } => Box::pin(async move {
            let _ = reply
                .send(connections::names(&database.connection, turn, &entries, &captured).await);
        }),
        Operation::ConnectionTrust {
            turn,
            package,
            inspection,
            captured,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                connections::trust(&database.connection, turn, &package, inspection, &captured)
                    .await,
            );
        }),
        Operation::Attachment { turn, id, reply } => Box::pin(async move {
            let result = match runs::get(&database.connection, turn).await {
                Ok(run) if run.status == Status::Running => {
                    super::attachments::turns::source(&database.connection, turn, id).await
                }
                Ok(_) => Err(Fault::new(
                    ErrorCode::Conflict,
                    "attachment request is no longer running",
                )),
                Err(error) => Err(error),
            };
            let _ = reply.send(result);
        }),
        Operation::Image {
            session,
            image,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(images::resolve(&database.connection, session, &image).await);
        }),
        Operation::Delegate {
            turn,
            request,
            arguments,
            message,
            token,
            reply,
        } => Box::pin(async move {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                Err(super::unavailable())
            } else {
                delegation::admit(database, turn, &request, arguments, message, events).await
            };
            if let Ok(invocation) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }),
        Operation::ClaimChild { turn, token, reply } => Box::pin(async move {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                let status = if controls.stopped.is_cancelled() {
                    Status::Interrupted
                } else {
                    Status::Cancelled
                };
                delegation::finish_queued(database, turn, status, None, events).await
            } else {
                delegation::claim(database, turn, events).await
            };
            if matches!(result, Ok(status) if status != Status::Queued) {
                controls.wake.notify_waiters();
            }
            let _ = reply.send(result);
        }),
        Operation::FinishQueuedChild {
            turn,
            status,
            error,
            reply,
        } => Box::pin(async move {
            let status = if controls.stopped.is_cancelled() {
                Status::Interrupted
            } else {
                status
            };
            let result = delegation::finish_queued(database, turn, status, error, events).await;
            controls.wake.notify_waiters();
            let _ = reply.send(result);
        }),
        Operation::CommandFinished(completed) => Box::pin(async move {
            let publication = database.publication.clone();
            let result = (async || -> Result<(), Fault> {
                let tx = database
                    .connection
                    .unchecked_transaction()
                    .await
                    .map_err(storage_error)?;
                let task = background::command_finished(
                    &tx,
                    &completed.snapshot,
                    completed.elapsed_ms,
                    completed.explicit,
                )
                .await?;
                let scheduled =
                    background::schedule(&tx, database.node, completed.snapshot.info.session)
                        .await?;
                let _publication = publication.lock().await;
                super::database::conflict::commit(tx).await?;
                background::publish(database, task).await;
                if let Some(turn) = scheduled {
                    feed::run(database, runs::get(&database.connection, turn.id).await?).await;
                    database.feeds.publish(
                        database.node,
                        turn.session,
                        Change::Queue(queue::read(&database.connection, turn.session).await?),
                    );
                    background::publish(
                        database,
                        background::list(&database.connection, turn.session).await?,
                    )
                    .await;
                }
                Ok(())
            })()
            .await;
            if let Err(error) = result {
                eprintln!("background completion persistence failed: {error}");
            }
        }),
        Operation::ConsumeCommand {
            parent,
            snapshot,
            reply,
        } => Box::pin(async move {
            let publication = database.publication.clone();
            let result = (async || -> Result<(), Fault> {
                let tx = database
                    .connection
                    .unchecked_transaction()
                    .await
                    .map_err(storage_error)?;
                let tasks = background::consume_command(&tx, parent, &snapshot).await?;
                let _publication = publication.lock().await;
                super::database::conflict::commit(tx).await?;
                background::publish(database, tasks).await;
                Ok(())
            })()
            .await;
            let _ = reply.send(result);
        }),
        Operation::CollectChild {
            parent,
            child,
            turn,
            reply,
        } => Box::pin(async move {
            let publication = database.publication.clone();
            let result = (async || -> Result<(), Fault> {
                let tx = database
                    .connection
                    .unchecked_transaction()
                    .await
                    .map_err(storage_error)?;
                let tasks = background::consume_child(&tx, parent, child, turn).await?;
                let _publication = publication.lock().await;
                super::database::conflict::commit(tx).await?;
                background::publish(database, tasks).await;
                Ok(())
            })()
            .await;
            let _ = reply.send(result);
        }),
        Operation::ChildState {
            parent,
            child,
            exact,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                delegation::control::state_at(&database.connection, parent, child, exact).await,
            );
        }),
        Operation::PendingGuidance { recipient, reply } => Box::pin(async move {
            let _ = reply.send(
                queue::guidance::pending(&database.connection, recipient)
                    .await
                    .map(|pending| pending.is_some()),
            );
        }),
        Operation::Checkpoint {
            turn,
            request,
            before,
            reply,
        } => Box::pin(async move {
            let _ =
                reply.send(checkpoints::capture(database, turn, &request, before.as_deref()).await);
        }),
        Operation::Ask {
            turn,
            request,
            reply,
        } => Box::pin(async move {
            match questions::begin(database, turn, &request, events).await {
                Ok(question) => {
                    database
                        .questions
                        .lock()
                        .unwrap()
                        .insert(question.id, (turn, reply));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
        }),
        Operation::ExpireQuestion {
            turn,
            call,
            stop,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(questions::expire(database, turn, &call, &stop, events).await);
        }),
        Operation::CancelInput { turn, call } => Box::pin(async move {
            if controls.stopped.is_cancelled() {
                return;
            }
            if let Err(error) = elicitation::cancel(database, turn, &call, events).await {
                eprintln!("MCP input persistence failed: {error}");
            }
        }),
        Operation::Confirm {
            turn,
            owner,
            deadline,
            request,
            binding,
            ssh,
            prepared,
            reply,
        } => Box::pin(async move {
            if reply.is_closed() || std::time::Instant::now() >= deadline {
                let _ = reply.send(Ok(Decision::Deny));
                return;
            }
            match approvals::begin_for(
                database,
                turn,
                &request,
                binding.as_ref(),
                approvals::Capture {
                    ssh: ssh.as_ref(),
                    prepared: prepared.as_ref(),
                },
                events,
            )
            .await
            {
                Ok(approval) if approval.state == ApprovalState::Approved => {
                    let _ = reply.send(Ok(Decision::Approve));
                }
                Ok(approval) => {
                    database
                        .approvals
                        .lock()
                        .unwrap()
                        .insert(approval.id, (turn, owner, deadline, Some(reply)));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
        }),
        #[cfg(any(test, feature = "test-support"))]
        Operation::ApprovalCommitFixture { gate, reply } => Box::pin(async move {
            *database.approval_commit.lock().unwrap() = Some(gate);
            let _ = reply.send(());
        }),
        Operation::CancelApproval { turn, call, owner } => Box::pin(async move {
            if controls.stopped.is_cancelled() {
                // Run completion owns the interrupted outcome during Node shutdown.
                return;
            }
            if let Err(error) = approvals::cancel(database, turn, &call, owner, events).await {
                eprintln!("approval cancellation persistence failed: {error}");
            }
        }),
        Operation::Authorize {
            turn,
            request,
            prepared,
            reply,
        } => Box::pin(async move {
            let _ =
                reply.send(approvals::authorize(database, turn, &request, prepared.as_ref()).await);
        }),
        Operation::ConnectionAccess {
            turn,
            request,
            prepared,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                approvals::connection_access(&database.connection, turn, &request, &prepared).await,
            );
        }),
        Operation::CommandAccess {
            turn,
            request,
            reply,
        } => Box::pin(async move {
            let _ =
                reply.send(approvals::command_access(&database.connection, turn, &request).await);
        }),
        Operation::Subscribe {
            session,
            closed,
            reply,
        } => Box::pin(async move {
            let publication = database.publication.clone();
            let feeds = database.feeds.clone();
            let node = database.node;
            let result = async {
                let publication = publication.lock().await;
                let transaction = database
                    .connection
                    .transaction_with_behavior(turso::transaction::TransactionBehavior::Deferred)
                    .await
                    .map_err(storage_error)?;
                transaction
                    .prepare("SELECT coalesce(max(cursor),0) FROM events")
                    .await
                    .map_err(storage_error)?
                    .query_row(())
                    .await
                    .map_err(storage_error)?;
                let reserved = feeds.reserve(session);
                drop(publication);
                let snapshot = async {
                    let history = read(&transaction, session, None, 20).await?;
                    let statistics = statistics::read(&transaction, session).await?;
                    let artifacts = super::artifacts::list(
                        &transaction,
                        session,
                        None,
                        sailry_protocol::artifact::MAX_PAGE_LIMIT,
                    )
                    .await?;
                    Ok::<_, Fault>((history, statistics, artifacts))
                }
                .await;
                match snapshot {
                    Ok((history, statistics, artifacts)) => {
                        crate::store::database::conflict::commit(transaction).await?;
                        reserved.subscribe(node, history, statistics, artifacts, closed)
                    }
                    Err(error) => {
                        transaction.rollback().await.map_err(storage_error)?;
                        Err(error)
                    }
                }
            }
            .await;
            let _ = reply.send(result);
        }),
        Operation::Get {
            turn,
            request,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(sessions::get(&database.connection, turn, request).await);
        }),
        Operation::Effects {
            session,
            turn,
            effects,
            presentation,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                sessions::effects(
                    database,
                    session,
                    turn,
                    effects,
                    presentation,
                    controls.timeline(turn),
                    events,
                )
                .await,
            );
        }),
        Operation::Append {
            session,
            turn,
            event,
            context,
            reply,
        } => Box::pin(async move {
            let _ = reply.send(
                history::append_with_context(database, session, turn, event, context, events).await,
            );
        }),
        Operation::Claim { token, reply } => Box::pin(async move {
            let result = if controls.stopped.is_cancelled() {
                Ok(None)
            } else {
                runs::claim(database, events).await
            };
            if let Ok(Some(invocation)) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }),
        Operation::ClaimGuidance {
            recipient,
            token,
            reply,
        } => Box::pin(async move {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                queue::guidance::retain(database, recipient, events)
                    .await
                    .map(|_| None)
            } else {
                runs::claim_guidance(database, recipient, events).await
            };
            if let Ok(Some(invocation)) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }),
        Operation::TakeGuidance { recipient, reply } => Box::pin(async move {
            let result = if controls.stopped.is_cancelled() || !controls.running(recipient) {
                Ok(None)
            } else {
                runs::guidance::take(database, recipient, events).await
            };
            let _ = reply.send(result);
        }),
        Operation::RetainGuidance { recipient, reply } => Box::pin(async move {
            let _ = reply.send(queue::guidance::retain(database, recipient, events).await);
        }),
        Operation::Finish {
            turn,
            status,
            error,
            owner,
            review,
            reply,
        } => Box::pin(async move {
            let result = (async || {
                let owner_stopped = owner
                    .as_ref()
                    .is_some_and(sailry_link::CancellationToken::is_cancelled);
                if owner_stopped {
                    controls.cancel(turn);
                    queue::guidance::retain(database, turn, events).await?;
                }
                // Resolve after every earlier Stop in this actor, not from a
                // Runner's stale cancellation observation or queue contents.
                let status = if controls.stopped.is_cancelled() || status == Status::Interrupted {
                    Status::Interrupted
                } else if owner_stopped {
                    Status::Cancelled
                } else {
                    status
                };
                let error = if status == Status::Interrupted
                    && error
                        .as_ref()
                        .is_none_or(|error| error.code != ErrorCode::OutcomeUnknown)
                {
                    Some(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "Node stopped execution; calls were not replayed",
                    ))
                } else {
                    error
                };
                runs::finish_with(database, turn, status, error, review, events).await
            })()
            .await;
            let _ = reply.send(result);
        }),
    }
}

async fn changed(
    db: &Connection,
    node: NodeId,
    session: SessionId,
) -> Result<EventEnvelope, Fault> {
    let event = activity::changed(db, session).await?;
    super::database::record(db, node, vec![event])
        .await?
        .pop()
        .ok_or_else(|| storage_error("event was not recorded"))
}

async fn check_session(db: &Connection, session: SessionId) -> Result<(), Fault> {
    let exists = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND state!='removed')")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get::<bool>(0);
        mapped.map_err(storage_error)?
    };
    if exists {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::NotFound,
            "session does not exist on this Node",
        ))
    }
}
