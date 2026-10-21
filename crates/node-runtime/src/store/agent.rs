//! ADK uses the existing Node storage worker, never a second conversation store.
pub(super) mod activity;
pub(super) mod approvals;
pub(super) mod assets;
pub(in crate::store) mod background;
mod calls;
pub(super) mod checkpoints;
mod children;
pub(crate) mod compaction;
pub(crate) mod connections;
pub(in crate::store) mod continuation;
pub(in crate::store) mod delegation;
pub(crate) mod elicitation;
mod feed;
mod history;
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
#[cfg(test)]
mod tests;
mod worktrees;

use super::{
    Database, Ingress, Job,
    database::{encode, storage_error},
};
use adk_core::{AdkError, Event as AdkEvent};
use adk_session::{GetRequest, Session as AdkSession};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{conversation::*, *};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};

pub(crate) use approvals::Binding as ApprovalBinding;
pub(crate) use approvals::results::{Denial, enrich as enrich_denials};
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
pub(crate) use sessions::{INPUT_PART, INPUT_SOURCE, INPUT_TURN, Sessions};

pub(crate) const SESSION_TITLE: &str = "sailry.session_title";

pub(crate) const APP: &str = "sailry";
pub(crate) const USER: &str = "node";

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
        reply: oneshot::Sender<Result<attachment::Attachment, Fault>>,
    },
    Image {
        session: SessionId,
        image: Image,
        reply: oneshot::Sender<Result<Vec<u8>, Fault>>,
    },
    Delegate {
        turn: TurnId,
        request: adk_core::ToolConfirmationRequest,
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
        request: adk_core::ToolConfirmationRequest,
        reply: oneshot::Sender<Result<question::Response, Fault>>,
    },
    CancelInput {
        turn: TurnId,
        call: String,
    },
    Confirm {
        turn: TurnId,
        owner: RequestId,
        request: adk_core::ToolConfirmationRequest,
        binding: Option<ApprovalBinding>,
        startup: Option<sailry_protocol::code::Startup>,
        ssh: Option<sailry_protocol::ssh::Trust>,
        prepared: Option<Command>,
        reply: oneshot::Sender<Result<Decision, Fault>>,
    },
    CancelApproval {
        turn: TurnId,
        call: String,
        owner: RequestId,
    },
    Authorize {
        turn: TurnId,
        request: adk_core::ToolConfirmationRequest,
        prepared: Option<Command>,
        reply: oneshot::Sender<Result<RequestId, Fault>>,
    },
    Denials {
        turn: TurnId,
        reply: oneshot::Sender<Result<Vec<Denial>, Fault>>,
    },
    ConnectionAccess {
        turn: TurnId,
        request: adk_core::ToolConfirmationRequest,
        prepared: Command,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    CommandAccess {
        turn: TurnId,
        request: adk_core::ToolConfirmationRequest,
        reply: oneshot::Sender<Result<process::Access, Fault>>,
    },
    Subscribe {
        session: SessionId,
        closed: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Box<dyn sailry_link::Subscription>, Fault>>,
    },
    ContextHistory {
        session: SessionId,
        turn: TurnId,
        reply: oneshot::Sender<adk_core::Result<compaction::History>>,
    },
    ContextUsage {
        session: SessionId,
        reply: oneshot::Sender<Result<Option<ContextUsage>, Fault>>,
    },
    Get {
        turn: TurnId,
        request: GetRequest,
        reply: oneshot::Sender<adk_core::Result<Box<dyn AdkSession>>>,
    },
    Append {
        session: SessionId,
        turn: TurnId,
        event: AdkEvent,
        reply: oneshot::Sender<adk_core::Result<()>>,
    },
    Claim {
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Option<Invocation>, Fault>>,
    },
    YieldGuidance {
        recipient: TurnId,
        reply: oneshot::Sender<Result<bool, Fault>>,
    },
    ClaimGuidance {
        recipient: TurnId,
        token: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Option<Invocation>, Fault>>,
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
        review: Option<AdkEvent>,
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
    pub child: Option<Child>,
    pub plugins: Vec<plugin::Info>,
    pub plugin_settings:
        std::collections::BTreeMap<String, Result<crate::plugins::settings::Resolved, Fault>>,
}

pub(crate) struct Child {
    pub role: Option<role::Profile>,
}

pub(crate) struct ContextUsage {
    pub observed: bool,
    pub tokens: u64,
    pub pending: Vec<AdkEvent>,
}

impl Ingress {
    pub(super) async fn tool_image(
        &self,
        session: SessionId,
        image: Image,
    ) -> Result<Vec<u8>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Image {
                session,
                image,
                reply,
            })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn context_history(
        &self,
        session: SessionId,
        turn: TurnId,
    ) -> adk_core::Result<compaction::History> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ContextHistory {
                session,
                turn,
                reply,
            })))
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?;
        response
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?
    }

    pub(crate) async fn context_usage(
        &self,
        session: SessionId,
    ) -> Result<Option<ContextUsage>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ContextUsage {
                session,
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
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Attachment {
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
        self.sender
            .send(Job::Agent(Box::new(Operation::Subscribe {
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
        event: AdkEvent,
    ) -> Result<(), Fault> {
        if self.closed.is_cancelled() {
            return Err(super::unavailable());
        }
        if !event.llm_response.partial || event.id.is_empty() || event.id.len() > 256 {
            return Err(Fault::invalid("invalid streaming event"));
        }
        let references = references::project(&event)?;
        let parts = event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
            .skip(usize::from(!references.is_empty()))
            .filter_map(|part| match part {
                adk_core::Part::Text { text } => Some(Part::Text(text.clone())),
                adk_core::Part::Thinking { thinking, .. } => Some(Part::Thinking(thinking.clone())),
                _ => None,
            })
            .collect();
        self.agents.stream(turn, session, || {
            self.feeds.delta(
                self.node,
                session,
                Draft {
                    id: event.id,
                    turn,
                    author: event.author,
                    branch: event.branch,
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
        self.sender
            .send(Job::Agent(Box::new(Operation::Claim { token, reply })))
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
        review: Option<AdkEvent>,
    ) -> Result<Status, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Finish {
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

    pub(crate) async fn yield_guidance(&self, recipient: TurnId) -> Result<bool, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::YieldGuidance {
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
        self.sender
            .send(Job::Agent(Box::new(Operation::ClaimGuidance {
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
        self.sender
            .send(Job::Agent(Box::new(Operation::RetainGuidance {
                recipient,
                reply,
            })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }
}

pub(super) fn execute(
    database: &mut Database,
    operation: Operation,
    events: &broadcast::Sender<EventEnvelope>,
    controls: &crate::agent::Controls,
) {
    match operation {
        Operation::ClaimHook {
            turn,
            phase,
            ordinal,
            reply,
        } => {
            let _ = reply.send(super::hooks::claim(
                &database.connection,
                turn,
                phase,
                ordinal,
            ));
        }
        Operation::FinishHook {
            turn,
            phase,
            ordinal,
            result,
            reply,
        } => {
            let _ = reply.send(super::hooks::finish(
                &database.connection,
                turn,
                phase,
                ordinal,
                &result,
            ));
        }
        Operation::Worktrees { turn, reply } => {
            let _ = reply.send(worktrees::list(database, turn));
        }
        Operation::Connection {
            turn,
            package,
            target,
            action,
            captured,
            reply,
        } => {
            let _ = reply.send(connections::command(
                &database.connection,
                turn,
                &package,
                target,
                action,
                &captured,
            ));
        }
        Operation::ConnectionStatus {
            turn,
            package,
            targets,
            captured,
            reply,
        } => {
            let _ = reply.send(connections::status(
                &database.connection,
                turn,
                &package,
                &targets,
                &captured,
            ));
        }
        Operation::ConnectionNames {
            turn,
            entries,
            captured,
            reply,
        } => {
            let _ = reply.send(connections::names(
                &database.connection,
                turn,
                &entries,
                &captured,
            ));
        }
        Operation::ConnectionTrust {
            turn,
            package,
            inspection,
            captured,
            reply,
        } => {
            let _ = reply.send(connections::trust(
                &database.connection,
                turn,
                &package,
                inspection,
                &captured,
            ));
        }
        Operation::Attachment { turn, id, reply } => {
            let result = calls::active(&database.connection, turn)
                .and_then(|_| super::attachments::turns::resolve(&database.connection, turn, id));
            let _ = reply.send(result);
        }
        Operation::Image {
            session,
            image,
            reply,
        } => {
            let _ = reply.send(images::resolve(&database.connection, session, &image));
        }
        Operation::Delegate {
            turn,
            request,
            arguments,
            message,
            token,
            reply,
        } => {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                Err(super::unavailable())
            } else {
                delegation::admit(database, turn, &request, arguments, message, events)
            };
            if let Ok(invocation) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }
        Operation::ClaimChild { turn, token, reply } => {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                let status = if controls.stopped.is_cancelled() {
                    Status::Interrupted
                } else {
                    Status::Cancelled
                };
                delegation::finish_queued(database, turn, status, None, events)
            } else {
                delegation::claim(database, turn, events)
            };
            if matches!(result, Ok(status) if status != Status::Queued) {
                controls.wake.notify_waiters();
            }
            let _ = reply.send(result);
        }
        Operation::FinishQueuedChild {
            turn,
            status,
            error,
            reply,
        } => {
            let status = if controls.stopped.is_cancelled() {
                Status::Interrupted
            } else {
                status
            };
            let result = delegation::finish_queued(database, turn, status, error, events);
            controls.wake.notify_waiters();
            let _ = reply.send(result);
        }
        Operation::CommandFinished(completed) => {
            let result = (|| -> Result<(), Fault> {
                let tx = database.connection.transaction().map_err(storage_error)?;
                let task = background::command_finished(
                    &tx,
                    &completed.snapshot,
                    completed.elapsed_ms,
                    completed.explicit,
                )?;
                let scheduled =
                    background::schedule(&tx, database.node, completed.snapshot.info.session)?;
                tx.commit().map_err(storage_error)?;
                background::publish(database, task);
                if let Some(turn) = scheduled {
                    feed::run(database, runs::get(&database.connection, turn.id)?);
                    database.feeds.publish(
                        database.node,
                        turn.session,
                        Change::Queue(queue::read(&database.connection, turn.session)?),
                    );
                    background::publish(
                        database,
                        background::list(&database.connection, turn.session)?,
                    );
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("background completion persistence failed: {error}");
            }
        }
        Operation::ConsumeCommand {
            parent,
            snapshot,
            reply,
        } => {
            let result = (|| -> Result<(), Fault> {
                let tx = database.connection.transaction().map_err(storage_error)?;
                let tasks = background::consume_command(&tx, parent, &snapshot)?;
                tx.commit().map_err(storage_error)?;
                background::publish(database, tasks);
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Operation::CollectChild {
            parent,
            child,
            turn,
            reply,
        } => {
            let result = (|| -> Result<(), Fault> {
                let tx = database.connection.transaction().map_err(storage_error)?;
                let tasks = background::consume_child(&tx, parent, child, turn)?;
                tx.commit().map_err(storage_error)?;
                background::publish(database, tasks);
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Operation::ChildState {
            parent,
            child,
            exact,
            reply,
        } => {
            let _ = reply.send(delegation::control::state_at(
                &database.connection,
                parent,
                child,
                exact,
            ));
        }
        Operation::PendingGuidance { recipient, reply } => {
            let _ = reply.send(
                queue::guidance::pending(&database.connection, recipient)
                    .map(|pending| pending.is_some()),
            );
        }
        Operation::Checkpoint {
            turn,
            request,
            before,
            reply,
        } => {
            let _ = reply.send(checkpoints::capture(
                database,
                turn,
                &request,
                before.as_deref(),
            ));
        }
        Operation::Ask {
            turn,
            request,
            reply,
        } => match questions::begin(database, turn, &request, events) {
            Ok(question) => {
                database.questions.insert(question.id, (turn, reply));
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        },
        Operation::CancelInput { turn, call } => {
            if let Err(error) = elicitation::cancel(database, turn, &call, events) {
                eprintln!("MCP input persistence failed: {error}");
            }
        }
        Operation::Confirm {
            turn,
            owner,
            request,
            binding,
            startup,
            ssh,
            prepared,
            reply,
        } => {
            if reply.is_closed() {
                return;
            }
            match approvals::begin_for(
                database,
                turn,
                &request,
                binding.as_ref(),
                approvals::Capture {
                    startup: startup.as_ref(),
                    ssh: ssh.as_ref(),
                    prepared: prepared.as_ref(),
                },
                events,
            ) {
                Ok(approval) if approval.state == ApprovalState::Approved => {
                    let _ = reply.send(Ok(Decision::Approve));
                }
                Ok(approval) => {
                    database.approvals.insert(approval.id, (turn, owner, reply));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
        }
        Operation::CancelApproval { turn, call, owner } => {
            if let Err(error) = approvals::cancel(database, turn, &call, owner, events) {
                eprintln!("approval cancellation persistence failed: {error}");
            }
        }
        Operation::Authorize {
            turn,
            request,
            prepared,
            reply,
        } => {
            let _ = reply.send(approvals::authorize(
                &mut database.connection,
                turn,
                &request,
                prepared.as_ref(),
            ));
        }
        Operation::Denials { turn, reply } => {
            let _ = reply.send(approvals::results::read(&database.connection, turn));
        }
        Operation::ConnectionAccess {
            turn,
            request,
            prepared,
            reply,
        } => {
            let _ = reply.send(approvals::connection_access(
                &database.connection,
                turn,
                &request,
                &prepared,
            ));
        }
        Operation::CommandAccess {
            turn,
            request,
            reply,
        } => {
            let _ = reply.send(approvals::command_access(
                &database.connection,
                turn,
                &request,
            ));
        }
        Operation::Subscribe {
            session,
            closed,
            reply,
        } => {
            let result = read(&database.connection, session, None, 20).and_then(|history| {
                let statistics = statistics::read(&database.connection, session)?;
                database.feeds.subscribe(
                    database.node,
                    history,
                    statistics,
                    super::artifacts::list(
                        &database.connection,
                        session,
                        None,
                        sailry_protocol::artifact::MAX_PAGE_LIMIT,
                    )?,
                    closed,
                )
            });
            let _ = reply.send(result);
        }
        Operation::ContextHistory {
            session,
            turn,
            reply,
        } => {
            let _ = reply.send(compaction::history(&database.connection, session, turn));
        }
        Operation::ContextUsage { session, reply } => {
            let result = check_session(&database.connection, session)
                .and_then(|_| statistics::context_usage(&database.connection, session));
            let _ = reply.send(result);
        }
        Operation::Get {
            turn,
            request,
            reply,
        } => {
            let _ = reply.send(sessions::get(&database.connection, turn, request));
        }
        Operation::Append {
            session,
            turn,
            event,
            reply,
        } => {
            let _ = reply.send(history::append(database, session, turn, event, events));
        }
        Operation::Claim { token, reply } => {
            let result = if controls.stopped.is_cancelled() {
                Ok(None)
            } else {
                runs::claim(database, events)
            };
            if let Ok(Some(invocation)) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }
        Operation::YieldGuidance { recipient, reply } => {
            let result = if controls.stopped.is_cancelled() {
                Ok(false)
            } else {
                queue::guidance::yield_run(database, recipient, events)
            };
            if matches!(result, Ok(true)) {
                controls.yield_run(recipient);
            } else if matches!(result, Ok(false)) && !controls.stopped.is_cancelled() {
                // Publish the model phase before replying. A guidance command admitted
                // next must interrupt even if the before-model callback has not resumed.
                controls.model_started(recipient);
            }
            let _ = reply.send(result);
        }
        Operation::ClaimGuidance {
            recipient,
            token,
            reply,
        } => {
            let result = if controls.stopped.is_cancelled() || token.is_cancelled() {
                queue::guidance::retain(database, recipient, events).map(|_| None)
            } else {
                runs::claim_guidance(database, recipient, events)
            };
            if let Ok(Some(invocation)) = &result {
                controls.register(invocation.turn.id, invocation.turn.session, token);
            }
            let _ = reply.send(result);
        }
        Operation::RetainGuidance { recipient, reply } => {
            let _ = reply.send(queue::guidance::retain(database, recipient, events));
        }
        Operation::Finish {
            turn,
            status,
            error,
            owner,
            review,
            reply,
        } => {
            let result = (|| {
                let owner_stopped = owner
                    .as_ref()
                    .is_some_and(sailry_link::CancellationToken::is_cancelled);
                if owner_stopped {
                    controls.cancel(turn);
                    queue::guidance::retain(database, turn, events)?;
                }
                // Resolve after every earlier Stop in this actor, not from a
                // Runner's stale cancellation observation or queue contents.
                let status = if controls.stopped.is_cancelled() || status == Status::Interrupted {
                    Status::Interrupted
                } else if owner_stopped {
                    Status::Cancelled
                } else if controls.yielded(turn) {
                    Status::Continued
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
                runs::finish_with(database, turn, status, error, review, events)
            })();
            let _ = reply.send(result);
        }
    }
}

fn changed(db: &Connection, node: NodeId, session: SessionId) -> Result<EventEnvelope, Fault> {
    let event = activity::changed(db, session)?;
    db.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
        .map_err(storage_error)?;
    Ok(EventEnvelope {
        node,
        cursor: db.last_insert_rowid() as u64,
        event,
    })
}

fn adk_error(error: Fault) -> AdkError {
    AdkError::session(error.to_string())
}

fn check_session(db: &Connection, session: SessionId) -> Result<(), Fault> {
    if db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND state!='removed')",
            [session.to_string()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)?
    {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::NotFound,
            "session does not exist on this Node",
        ))
    }
}
