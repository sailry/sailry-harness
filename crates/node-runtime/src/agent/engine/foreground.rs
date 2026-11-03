use super::{Effect, Session};
use crate::store::Ingress;
use async_trait::async_trait;
use goose_agent::{
    operation::{Emitter, Operation, OperationResult, not_applicable},
    subagent::SubagentOperation,
};
use goose_provider_types::conversation::Conversation;
use sailry_protocol::TurnId;
use std::sync::Arc;

pub(super) struct Foreground {
    pub inner: SubagentOperation<Session>,
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
}

#[async_trait]
impl Operation<Session, Effect> for Foreground {
    fn name(&self) -> &'static str {
        "foreground_subagent"
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
        if goose_agent::operation::last_effective_role(
            goose_agent::operation::messages_since_kickoff(conversation)?,
        )? == goose_provider_types::conversation::EffectiveRole::User
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
