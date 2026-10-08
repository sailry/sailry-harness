//! ADK uses the existing Node storage worker, never a second conversation store.
pub(super) mod activity;
pub(super) mod approvals;
pub(super) mod assets;
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
    Worktrees {
        turn: TurnId,
        reply: oneshot::Sender<Result<Vec<Worktree>, Fault>>,
    },
    Connection {
        turn: TurnId,
        target: sailry_protocol::connection::Resource,
        action: connections::Action,
        reply: oneshot::Sender<Result<Command, Fault>>,
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
    ChildState {
        parent: TurnId,
        child: SessionId,
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
        request: adk_core::ToolConfirmationRequest,
        binding: Option<ApprovalBinding>,
        reply: oneshot::Sender<Result<Decision, Fault>>,
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
    Subscribe {
        session: SessionId,
        closed: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<Box<dyn sailry_link::Subscription>, Fault>>,
    },
    ContextUsage {
        session: SessionId,
        reply: oneshot::Sender<Result<Option<ContextUsage>, Fault>>,
    },
    Delta {
        session: SessionId,
        turn: TurnId,
        event: AdkEvent,
        reply: oneshot::Sender<Result<(), Fault>>,
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
        reply: oneshot::Sender<Result<Status, Fault>>,
    },
}

pub(crate) struct Invocation {
    pub automatic: bool,
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
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Delta {
                session,
                turn,
                event,
                reply,
            })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
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

    pub(crate) async fn finish_run(
        &self,
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
        owner: Option<sailry_link::CancellationToken>,
    ) -> Result<Status, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Finish {
                turn,
                status,
                error,
                owner,
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
        Operation::Worktrees { turn, reply } => {
            let _ = reply.send(worktrees::list(database, turn));
        }
        Operation::Connection {
            turn,
            target,
            action,
            reply,
        } => {
            let _ = reply.send(connections::command(
                &database.connection,
                turn,
                target,
                action,
            ));
        }
        Operation::Attachment { turn, id, reply } => {
            let _ = reply.send(super::attachments::turns::resolve(
                &database.connection,
                turn,
                id,
            ));
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
        Operation::ChildState {
            parent,
            child,
            reply,
        } => {
            let _ = reply.send(delegation::state(&database.connection, parent, child));
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
            request,
            binding,
            reply,
        } => match approvals::begin(database, turn, &request, binding.as_ref(), events) {
            Ok(approval) if approval.state == ApprovalState::Approved => {
                let _ = reply.send(Ok(Decision::Approve));
            }
            Ok(approval) => {
                database.approvals.insert(approval.id, (turn, reply));
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        },
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
        Operation::Subscribe {
            session,
            closed,
            reply,
        } => {
            let result = read(&database.connection, session, None, 20).and_then(|history| {
                let statistics = statistics::read(&database.connection, session)?;
                Ok(database
                    .feeds
                    .subscribe(database.node, history, statistics, closed))
            });
            let _ = reply.send(result);
        }
        Operation::ContextUsage { session, reply } => {
            let result = check_session(&database.connection, session)
                .and_then(|_| statistics::context_usage(&database.connection, session));
            let _ = reply.send(result);
        }
        Operation::Delta {
            session,
            turn,
            event,
            reply,
        } => {
            let result =
                history::project(&database.connection, 0, turn, &event).and_then(|entry| {
                    let parts = entry
                        .parts
                        .into_iter()
                        .filter(|part| matches!(part, Part::Text(_) | Part::Thinking(_)))
                        .collect();
                    database.feeds.delta(
                        database.node,
                        session,
                        Draft {
                            id: entry.id,
                            turn,
                            author: entry.author,
                            branch: entry.branch,
                            parts,
                        },
                    )
                });
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
                runs::finish(database, turn, status, error, events)
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
