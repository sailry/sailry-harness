//! Guidance inputs retain their own canonical user entries before one Runner call.
use super::*;
use adk_core::{Content, Event, Part};
use adk_session::GetRequest;
use sailry_protocol::conversation::{QueuedMessage, reference::Reference};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Batch {
    entries: Vec<Entry>,
}

#[derive(Clone)]
struct Entry {
    turn: TurnId,
    references: Vec<Reference>,
    content: Content,
    capability: Option<usize>,
}

impl Batch {
    pub async fn select(
        &mut self,
        invocation: &Invocation,
        resources: &crate::plugins::resources::Resources,
        tools: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), Fault> {
        for entry in &mut self.entries {
            let instruction = references::capabilities::instruction(
                invocation,
                entry.references.iter(),
                resources,
                tools,
            )
            .await?;
            if !instruction.is_empty() {
                entry.capability = Some(entry.content.parts.len());
                entry.content.parts.push(Part::Text { text: instruction });
            }
        }
        Ok(())
    }

    pub async fn capture(ingress: &Ingress, invocation: &Invocation) -> Result<Self, Fault> {
        let mut entries = vec![Entry {
            turn: invocation.input_turn.unwrap_or(invocation.turn.id),
            references: invocation.message.references.clone(),
            content: attachments::input(ingress, invocation).await?,
            capability: None,
        }];
        for input in &invocation.followups {
            entries.push(Entry {
                turn: input.turn.id,
                references: input.message.references.clone(),
                content: attachments::message(ingress, invocation.turn.id, &input.message, None)
                    .await?,
                capability: None,
            });
        }
        Ok(Self { entries })
    }

    pub async fn guidance(
        ingress: &Ingress,
        owner: TurnId,
        inputs: &[QueuedMessage],
    ) -> Result<Self, Fault> {
        let mut entries = Vec::with_capacity(inputs.len());
        for input in inputs {
            entries.push(Entry {
                turn: input.turn.id,
                references: input.message.references.clone(),
                content: attachments::message(ingress, owner, &input.message, None).await?,
                capability: None,
            });
        }
        Ok(Self { entries })
    }

    pub fn current(&self) -> (TurnId, Vec<Reference>, Content, Option<usize>) {
        let entry = self.entries.last().expect("an invocation has input");
        (
            entry.turn,
            entry.references.clone(),
            entry.content.clone(),
            entry.capability,
        )
    }

    pub fn pending(&self) -> Vec<Content> {
        self.entries
            .iter()
            .map(|entry| entry.content.clone())
            .collect()
    }

    pub async fn append(
        &self,
        ingress: &Arc<Ingress>,
        target: SessionId,
        owner: TurnId,
        invocation: &str,
        source: &'static str,
        omit: Option<TurnId>,
        mut context: Option<Event>,
    ) -> Result<(), Fault> {
        if context.is_none() && self.entries.iter().all(|entry| omit == Some(entry.turn)) {
            return Ok(());
        }
        let sessions = ingress.sessions(owner);
        let session = sessions
            .get(GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: target.to_string(),
                num_recent_events: None,
                after: None,
            })
            .await
            .map_err(model::error)?;
        let state = session.state().all();
        if context.as_ref().is_some_and(|context| {
            !context.actions.state_delta.is_empty()
                && context
                    .actions
                    .state_delta
                    .iter()
                    .all(|(key, value)| state.get(key) == Some(value))
        }) {
            // A partially saved batch already committed its context with the
            // first input. Retrying must not append it again after that input.
            context = None;
        }
        let present: BTreeSet<_> = session
            .events()
            .all()
            .iter()
            .filter(|event| event.author == "user")
            .filter_map(|event| {
                event
                    .provider_metadata
                    .get(crate::store::agent::INPUT_TURN)
                    .cloned()
            })
            .collect();
        for entry in &self.entries {
            if omit == Some(entry.turn) || present.contains(&entry.turn.to_string()) {
                continue;
            }
            let mut event = Event::new(invocation);
            event.author = "user".into();
            event.set_content(entry.content.clone());
            crate::store::agent::Sessions::new(ingress.clone(), owner)
                .with_input_turn(entry.turn)
                .with_input_source(source)
                .with_references(entry.references.clone())
                .with_capability_part(entry.capability)
                .append_with_context(&target.to_string(), event, context.take())
                .await
                .map_err(model::error)?;
        }
        if let Some(context) = context {
            crate::store::agent::Sessions::new(ingress.clone(), owner)
                .append_with_context(&target.to_string(), context, None)
                .await
                .map_err(model::error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
