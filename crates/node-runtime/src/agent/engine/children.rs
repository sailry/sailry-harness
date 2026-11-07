//! Foreground execution consumes the Node's already admitted child exactly once.
use crate::agent::model::{Content, Message};
use crate::{
    agent::delegation,
    store::{
        Ingress,
        agent::{Invocation, RuntimeSession},
    },
    tasks::Tasks,
};
use futures::future::BoxFuture;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, SessionId, conversation::Status};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(crate) struct Children {
    ingress: Arc<Ingress>,
    tasks: Tasks,
    parent: SessionId,
    turn: sailry_protocol::TurnId,
    pending: Arc<Mutex<BTreeMap<String, Invocation>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ChildOutcome {
    Completed(String),
    Failed(String),
    Cancelled,
}
impl Children {
    pub(crate) fn new(ingress: Arc<Ingress>, tasks: Tasks, invocation: &Invocation) -> Self {
        Self {
            ingress,
            tasks,
            parent: invocation.turn.session,
            turn: invocation.turn.id,
            pending: Arc::default(),
        }
    }
    pub(crate) fn register(&self, invocation: Invocation) -> Result<(), Fault> {
        let mut pending = self.pending.lock().unwrap();
        let id = invocation.turn.session.to_string();
        if pending.contains_key(&id) {
            return Err(Fault::invalid("child invocation is already registered"));
        }
        pending.insert(id, invocation);
        Ok(())
    }
    pub(in crate::agent) async fn settle_pending(&self) -> Result<(), Fault> {
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (_, invocation) in pending {
            crate::agent::settlement::queued(
                &self.ingress,
                invocation.turn.id,
                invocation.turn.session,
                Status::Cancelled,
                None,
            )
            .await?;
            self.ingress.agents.finish(invocation.turn.id);
        }
        Ok(())
    }
}
struct Owner {
    ingress: Arc<Ingress>,
    parent: SessionId,
    child: SessionId,
    turn: sailry_protocol::TurnId,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.ingress
            .agents
            .finish_child(self.parent, self.child, self.turn);
    }
}
impl Children {
    pub(in crate::agent) fn requested_subagent_session_ids(
        &self,
        session: &RuntimeSession,
        message: &Message,
    ) -> Vec<String> {
        if !session.records.iter().any(|record| {
            record.turn == self.turn && message.id.as_deref() == Some(record.id.as_str())
        }) {
            return Vec::new();
        }
        message
            .content
            .iter()
            .filter_map(|content| {
                let Content::ToolResult(response) = content else {
                    return None;
                };
                let meta = response.result.as_ref().ok()?.meta.as_ref()?;
                (meta
                    .get("foreground_subagent")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
                .then(|| {
                    meta.get("subagent_session_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .flatten()
            })
            .collect()
    }
    pub(super) fn start(
        &self,
        id: &str,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, ChildOutcome> {
        let this = self.clone();
        let id = id.to_owned();
        Box::pin(async move {
            let owner = cancel.child_token();
            let observer = owner.clone().drop_guard();
            let execute_owner = owner.clone();
            let task = this
                .tasks
                .clone()
                .spawn_child(&this.ingress.agents.stopped.clone(), async move {
                    let invocation = this.pending.lock().unwrap().remove(&id);
                    let Some(invocation) = invocation else {
                        return ChildOutcome::Failed(
                            "child invocation is unavailable; execution was not replayed".into(),
                        );
                    };
                    let owner = execute_owner;
                    this.ingress.agents.register_child(
                        this.parent,
                        invocation.turn.session,
                        invocation.turn.id,
                        owner.clone(),
                    );
                    let _owner = Owner {
                        ingress: this.ingress.clone(),
                        parent: this.parent,
                        child: invocation.turn.session,
                        turn: invocation.turn.id,
                    };
                    let (_, result) = delegation::scheduler::resume(
                        &this.ingress,
                        &this.tasks,
                        invocation,
                        owner.child_token(),
                        owner.clone(),
                    )
                    .await;
                    let outcome = match result {
                        Ok((Status::Completed, output)) => {
                            ChildOutcome::Completed(output.unwrap_or_default())
                        }
                        Ok((Status::Cancelled | Status::Interrupted, _)) => ChildOutcome::Cancelled,
                        Ok((status, _)) => {
                            ChildOutcome::Failed(format!("child ended with {status:?}"))
                        }
                        Err(error) => ChildOutcome::Failed(error.message),
                    };
                    if let Ok(session) = id.parse()
                        && let Ok(state) = this.ingress.child_state(this.turn, session).await
                    {
                        let _ = this
                            .ingress
                            .collect_child(this.turn, session, state.run.turn)
                            .await;
                    }
                    outcome
                })
                .await;
            let _observer = observer;
            match task {
                Ok(task) => task.await.unwrap_or_else(|_| {
                    ChildOutcome::Failed("child execution outcome is unavailable".into())
                }),
                Err(_) => ChildOutcome::Failed("child supervisor is unavailable".into()),
            }
        })
    }
}
