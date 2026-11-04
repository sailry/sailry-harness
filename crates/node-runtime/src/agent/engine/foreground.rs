use super::{Effect, Session};
use crate::store::Ingress;
use async_trait::async_trait;
use goose_agent::{
    operation::{Emitter, Operation, OperationResult, not_applicable},
    subagent::SubagentOperation,
};
use goose_provider_types::conversation::{Conversation, EffectiveRole, message::Message};
use sailry_protocol::TurnId;
use std::sync::Arc;

const OPERATION: &str = "foreground_subagent";

pub(super) struct Foreground {
    pub inner: SubagentOperation<Session>,
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
}

#[async_trait]
impl Operation<Session, Effect> for Foreground {
    fn name(&self) -> &'static str {
        OPERATION
    }

    async fn finalize_cancellation(
        &self,
        session: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> Vec<Effect> {
        self.inner
            .finalize_cancellation(session, conversation, emit)
            .await
    }

    async fn run(
        &self,
        session: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        let messages = goose_agent::operation::messages_since_kickoff(conversation)?;
        if goose_agent::operation::last_effective_role(messages)? == EffectiveRole::User
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
            result = self.inner.run(session, conversation, emit) => result,
        }
    }
}

/// GDK delivers each finished foreground child as a user message carrying this
/// operation's `delivered` or `cancelled` note. Such deliveries are not new user
/// input, so the remaining children must still be awaited before inference.
fn delivers_child(message: &Message) -> bool {
    ["delivered", "cancelled"]
        .into_iter()
        .any(|key| message.metadata.operation_note(OPERATION, key).is_some())
}
