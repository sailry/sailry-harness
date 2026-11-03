use super::{Effect, Session};
use crate::agent::controls::Controls;
use async_trait::async_trait;
use goose_agent::{
    operation::{Emitter, Operation, OperationResult},
    tool::ToolOperation,
};
use goose_provider_types::conversation::Conversation;
use sailry_protocol::TurnId;
use std::sync::Arc;

pub(super) struct Tools {
    pub inner: ToolOperation<Session>,
    pub controls: Arc<Controls>,
    pub turn: TurnId,
}

#[async_trait]
impl Operation<Session, Effect> for Tools {
    fn name(&self) -> &'static str {
        "tools"
    }

    async fn inference_tools(&self, session: &Session) -> anyhow::Result<Vec<rmcp::model::Tool>> {
        <ToolOperation<Session> as Operation<Session, Effect>>::inference_tools(
            &self.inner,
            session,
        )
        .await
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
        let token = self
            .controls
            .tool_token(self.turn)
            .unwrap_or_else(|| emit.cancel_token().clone());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let scoped = Emitter::new(sender, token);
        let work = self.inner.run(session, conversation, &scoped);
        tokio::pin!(work);
        let result = loop {
            tokio::select! {
                result = &mut work => break result,
                Some(event) = events.recv() => emit.emit(event),
            }
        };
        while let Ok(event) = events.try_recv() {
            emit.emit(event);
        }
        result
    }
}
