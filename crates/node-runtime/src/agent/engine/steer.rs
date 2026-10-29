//! Adapted from Goose ee8500d5, ops_steer.rs (Apache-2.0).
//! Node owns admission and recovery of every drained input.
use super::{Effect, Session};
use crate::{
    agent::{
        canonical::{Configuration, Context, Guide, Kind, Record},
        inputs,
        prompt::instructions::{self, Scope},
    },
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
        session: &Session,
        conversation: &Conversation,
        emit: &Emitter,
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
        let mut records = batch.records("user", true);
        if self.invocation.project.is_some() && self.invocation.connections.bound.is_none() {
            use sailry_protocol::conversation::reference::Target;
            let scopes: Vec<_> = inputs
                .iter()
                .flat_map(|input| &input.message.references)
                .filter_map(|reference| match &reference.target {
                    Target::File(path) => Some(Scope::File(path.clone())),
                    Target::Directory(path) => Some(Scope::Directory(path.clone())),
                    _ => None,
                })
                .collect();
            let mut context = Context {
                captured: true,
                ..Default::default()
            };
            for guide in instructions::capture(
                &self.ingress,
                self.invocation.worktree,
                &scopes,
                emit.cancel_token(),
            )
            .await?
            {
                let key = guide.key();
                // Existing guides stay frozen for this turn; guidance can add a new scope.
                if !session.context.guides.contains_key(&key) {
                    context.guides.insert(
                        key,
                        Guide {
                            path: guide.path.clone(),
                            revision: guide.revision.clone(),
                            content: guide.render(),
                        },
                    );
                }
            }
            if !context.guides.is_empty() {
                let mut record = Record::new(
                    self.invocation.turn.id,
                    Kind::Configuration(Configuration { context }),
                );
                record.provenance.author = "system".into();
                records.push(record);
            }
        }
        // Cancellation may commit only input whose preparation fully succeeded.
        *self.drained.lock().unwrap() = records;
        applied([Effect::Records(std::mem::take(
            &mut *self.drained.lock().unwrap(),
        ))])
    }
}
