//! Adapted from Goose ee8500d5, ops_steer.rs (Apache-2.0).
//! Node owns admission and recovery of every drained input.
use super::{Effect, Session};
use crate::{
    agent::{canonical::Record, inputs},
    store::{Ingress, agent::Invocation},
};
use async_trait::async_trait;
use goose_agent::operation::*;
use goose_provider_types::conversation::{Conversation, EffectiveRole};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub(super) struct Steer {
    pub ingress: Arc<Ingress>,
    pub invocation: Arc<Invocation>,
    pub resources: Arc<crate::plugins::resources::Resources>,
    pub tools: BTreeMap<String, Vec<String>>,
    pub drained: Mutex<Vec<Record>>,
}
#[async_trait]
impl Operation<Session, Effect> for Steer {
    fn name(&self) -> &'static str {
        "steer"
    }
    async fn finalize_cancellation(
        &self,
        _: &Session,
        _: &Conversation,
        _: &Emitter,
    ) -> Vec<Effect> {
        let records = std::mem::take(&mut *self.drained.lock().unwrap());
        if records.is_empty() {
            Vec::new()
        } else {
            vec![Effect::Records(records)]
        }
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        _: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        let messages = messages_since_kickoff(conversation)?;
        if !ends_turn(messages) && last_effective_role(messages)? != EffectiveRole::Tool {
            return not_applicable();
        }
        let Some(inputs) = self.ingress.take_guidance(self.invocation.turn.id).await? else {
            return not_applicable();
        };
        let mut batch =
            inputs::Batch::guidance(&self.ingress, self.invocation.turn.id, &inputs).await?;
        batch
            .select(&self.invocation, &self.resources, &self.tools)
            .await?;
        *self.drained.lock().unwrap() = batch.records("user", true);
        applied([Effect::Records(std::mem::take(
            &mut *self.drained.lock().unwrap(),
        ))])
    }
}
