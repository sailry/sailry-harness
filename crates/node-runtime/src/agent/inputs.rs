//! Guidance inputs retain their own canonical user entries before one Runner call.
use super::*;
use adk_core::{Content, Event};
use adk_session::GetRequest;
use sailry_protocol::conversation::{QueuedMessage, reference::Reference};
use std::collections::BTreeSet;

pub(super) struct Batch {
    entries: Vec<(TurnId, Vec<Reference>, Content)>,
}

impl Batch {
    pub async fn capture(ingress: &Ingress, invocation: &Invocation) -> Result<Self, Fault> {
        let mut entries = vec![(
            invocation.input_turn.unwrap_or(invocation.turn.id),
            invocation.message.references.clone(),
            attachments::input(ingress, invocation).await?,
        )];
        for input in &invocation.followups {
            entries.push((
                input.turn.id,
                input.message.references.clone(),
                attachments::message(ingress, invocation.turn.id, &input.message, None).await?,
            ));
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
            entries.push((
                input.turn.id,
                input.message.references.clone(),
                attachments::message(ingress, owner, &input.message, None).await?,
            ));
        }
        Ok(Self { entries })
    }

    pub fn current(&self) -> (TurnId, Vec<Reference>, Content) {
        self.entries
            .last()
            .expect("an invocation has input")
            .clone()
    }

    pub fn pending(&self) -> Vec<Content> {
        self.entries
            .iter()
            .map(|(_, _, content)| content.clone())
            .collect()
    }

    pub async fn append(
        &self,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        source: &'static str,
    ) -> Result<(), Fault> {
        if self.entries.len() == 1 {
            return Ok(());
        }
        self.append_entries(
            ingress,
            invocation.turn.session,
            invocation.turn.id,
            &invocation.turn.id.to_string(),
            source,
            self.entries.len() - 1,
        )
        .await
    }

    pub async fn append_guidance(
        &self,
        ingress: &Arc<Ingress>,
        session: SessionId,
        owner: TurnId,
        invocation: &str,
        source: &'static str,
    ) -> Result<(), Fault> {
        self.append_entries(
            ingress,
            session,
            owner,
            invocation,
            source,
            self.entries.len(),
        )
        .await
    }

    async fn append_entries(
        &self,
        ingress: &Arc<Ingress>,
        target: SessionId,
        owner: TurnId,
        invocation: &str,
        source: &'static str,
        count: usize,
    ) -> Result<(), Fault> {
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
        for (turn, references, content) in &self.entries[..count] {
            if present.contains(&turn.to_string()) {
                continue;
            }
            if !ingress.agents.running(owner) {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "guidance recipient is no longer running",
                ));
            }
            let mut event = Event::new(invocation);
            event.author = "user".into();
            event.set_content(content.clone());
            crate::store::agent::Sessions::new(ingress.clone(), owner)
                .with_input_turn(*turn)
                .with_input_source(source)
                .with_references(references.clone())
                .append_event(&target.to_string(), event)
                .await
                .map_err(model::error)?;
        }
        Ok(())
    }
}
