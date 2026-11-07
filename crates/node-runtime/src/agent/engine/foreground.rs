//! Child delivery notes adapted from Goose 14741e9a subagent.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.
use super::{
    Children, Effect, Session,
    children::ChildOutcome,
    conversation::{EffectiveRole, last_effective_role, messages_since_kickoff},
    operation::*,
};
use crate::{agent::model::Message, store::Ingress};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use sailry_protocol::TurnId;
use std::{collections::BTreeSet, sync::Arc};

const OPERATION: &str = "foreground_subagent";

#[derive(Default)]
struct Running {
    ids: BTreeSet<String>,
    runs: FuturesUnordered<BoxFuture<'static, (String, ChildOutcome)>>,
}

pub(super) struct Foreground {
    children: Children,
    ingress: Arc<Ingress>,
    turn: TurnId,
    running: tokio::sync::Mutex<Running>,
}

impl Foreground {
    pub(super) fn new(children: Children, ingress: Arc<Ingress>, turn: TurnId) -> Self {
        Self {
            children,
            ingress,
            turn,
            running: Default::default(),
        }
    }

    fn pending(&self, session: &Session, messages: &[Message]) -> Vec<String> {
        let mut pending = Vec::new();
        for message in messages {
            let delivered = message
                .metadata
                .operation_note(OPERATION, "delivered")
                .and_then(serde_json::Value::as_str);
            let cancelled = message
                .metadata
                .operation_note(OPERATION, "cancelled")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str);
            for id in delivered.into_iter().chain(cancelled) {
                pending.retain(|candidate| candidate != id);
            }
            for id in self
                .children
                .requested_subagent_session_ids(session, message)
            {
                if !pending.contains(&id) {
                    pending.push(id);
                }
            }
        }
        pending
    }

    pub(super) async fn finalize_cancellation(&self, session: &Session) -> Vec<Effect> {
        let pending = self.pending(session, &session.conversation);
        let mut running = std::mem::take(&mut *self.running.lock().await);
        let mut delivered = BTreeSet::new();
        let mut effects = Vec::new();
        while let Some(Some((id, outcome))) = running.runs.next().now_or_never() {
            if outcome != ChildOutcome::Cancelled {
                delivered.insert(id.clone());
                effects.push(result(&id, outcome).into());
            }
        }
        drop(running);
        let cancelled: Vec<_> = pending
            .into_iter()
            .filter(|id| !delivered.contains(id))
            .collect();
        if !cancelled.is_empty() {
            effects.push(cancellation(&cancelled).into());
        }
        effects
    }

    async fn wait(
        &self,
        session: &Session,
        messages: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let mut running = self.running.lock().await;
        for id in self.pending(session, messages) {
            if running.ids.insert(id.clone()) {
                let child = self.children.start(&id, emit.cancel_token().clone());
                running
                    .runs
                    .push(Box::pin(async move { (id, child.await) }));
            }
        }
        if let Some((id, outcome)) = running.runs.next().await {
            running.ids.remove(&id);
            return applied([result(&id, outcome).into()]);
        }
        not_applicable()
    }

    pub(super) async fn run(
        &self,
        session: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let messages = messages_since_kickoff(conversation)?;
        if last_effective_role(messages)? == EffectiveRole::User
            && !messages.last().is_some_and(delivers_child)
        {
            return not_applicable();
        }
        let Some(notice) = self.ingress.agents.guidance_notice(self.turn) else {
            return not_applicable();
        };
        let guidance = async {
            loop {
                let notified = notice.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.ingress.pending_guidance(self.turn).await? {
                    return Ok::<_, sailry_protocol::Fault>(());
                }
                notified.await;
            }
        };
        tokio::select! {
            biased;
            result = guidance => {
                result?;
                not_applicable()
            }
            result = self.wait(session, conversation, emit) => result,
        }
    }
}

fn result(id: &str, outcome: ChildOutcome) -> Message {
    let text = match outcome {
        ChildOutcome::Completed(output) => format!("Subagent {id} completed: {output}"),
        ChildOutcome::Failed(reason) => format!("Subagent {id} failed: {reason}"),
        ChildOutcome::Cancelled => return cancellation(&[id.to_owned()]),
    };
    let mut message = Message::user().with_text(text).with_visibility(false, true);
    message
        .metadata
        .set_operation_note(OPERATION, "delivered", serde_json::json!(id));
    message
}

fn cancellation(ids: &[String]) -> Message {
    let text = ids
        .iter()
        .map(|id| format!("Subagent {id} was cancelled before it finished."))
        .collect::<Vec<_>>()
        .join("\n");
    let mut message = Message::user().with_text(text).with_visibility(false, true);
    message
        .metadata
        .set_operation_note(OPERATION, "cancelled", serde_json::json!(ids));
    message
}

fn delivers_child(message: &Message) -> bool {
    ["delivered", "cancelled"]
        .into_iter()
        .any(|key| message.metadata.operation_note(OPERATION, key).is_some())
}
