//! GDK foreground execution consumes the Node's already admitted child exactly once.
use crate::{
    agent::delegation,
    store::{
        Ingress,
        agent::{Invocation, RuntimeSession},
    },
    tasks::Tasks,
};
use async_trait::async_trait;
use futures::stream;
use goose_agent::{
    operation::OperationStream,
    subagent::{SubagentOutcome, SubagentRunner, SubagentUpdate},
};
use goose_provider_types::conversation::message::{Message, MessageContent};
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
    pub(super) async fn settle_pending(&self) -> Result<(), Fault> {
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (_, invocation) in pending {
            loop {
                match self
                    .ingress
                    .finish_queued_child(invocation.turn.id, Status::Cancelled, None)
                    .await
                {
                    Ok(_) => {
                        self.ingress.agents.finish(invocation.turn.id);
                        break;
                    }
                    Err(error) => {
                        if self.ingress.agents.stopped.is_cancelled() {
                            return Err(error);
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
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
#[async_trait]
impl SubagentRunner<RuntimeSession> for Children {
    fn requested_subagent_session_ids(&self, _: &RuntimeSession, message: &Message) -> Vec<String> {
        message
            .content
            .iter()
            .filter_map(|content| {
                let MessageContent::ToolResponse(response) = content else {
                    return None;
                };
                let meta = response.tool_result.as_ref().ok()?.meta.as_ref()?;
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
    async fn start(
        &self,
        _: &RuntimeSession,
        id: &str,
        cancel: CancellationToken,
    ) -> OperationStream<'static, SubagentUpdate> {
        let invocation = self.pending.lock().unwrap().remove(id);
        let this = self.clone();
        let id = id.to_owned();
        let owner = cancel.child_token();
        let observer = owner.clone().drop_guard();
        let execute_owner = owner.clone();
        let task = this
            .tasks
            .clone()
            .spawn_child(&this.ingress.agents.stopped.clone(), async move {
                let Some(mut invocation) = invocation else {
                    return SubagentOutcome::Failed(
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
                let result = loop {
                    let turn = invocation.turn.id;
                    let result = delegation::scheduler::run(
                        &this.ingress,
                        &this.tasks,
                        invocation,
                        owner.child_token(),
                        owner.clone(),
                    )
                    .await;
                    if result.is_err() || owner.is_cancelled() {
                        let _ = this.ingress.retain_guidance(turn).await;
                        break result;
                    }
                    match this.ingress.claim_guidance(turn, owner.child_token()).await {
                        Ok(Some(next)) => invocation = next,
                        Ok(None) => break result,
                        Err(error) => break Err(error),
                    }
                };
                let outcome = match result {
                    Ok((Status::Completed, output)) => {
                        SubagentOutcome::Completed(output.unwrap_or_default())
                    }
                    Ok((Status::Cancelled | Status::Interrupted, _)) => SubagentOutcome::Cancelled,
                    Ok((status, _)) => {
                        SubagentOutcome::Failed(format!("child ended with {status:?}"))
                    }
                    Err(error) => SubagentOutcome::Failed(error.message),
                };
                if let Ok(session) = id.parse() {
                    if let Ok(state) = this.ingress.child_state(this.turn, session).await {
                        let _ = this
                            .ingress
                            .collect_child(this.turn, session, state.run.turn)
                            .await;
                    }
                }
                outcome
            })
            .await;
        Box::pin(stream::once(async move {
            let _observer = observer;
            SubagentUpdate::Finished(match task {
                Ok(task) => task.await.unwrap_or_else(|_| {
                    SubagentOutcome::Failed("child execution outcome is unavailable".into())
                }),
                Err(_) => SubagentOutcome::Failed("child supervisor is unavailable".into()),
            })
        }))
    }
}
