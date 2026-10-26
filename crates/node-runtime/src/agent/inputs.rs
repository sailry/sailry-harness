//! Guidance inputs retain their own canonical user entries before one Runner call.
use super::*;
use adk_core::{Content, Event};
use adk_session::GetRequest;
use sailry_protocol::conversation::reference::Reference;
use std::collections::BTreeSet;

pub(super) struct Batch {
    entries: Vec<(TurnId, Vec<Reference>, Content)>,
}

impl Batch {
    pub async fn capture(ingress: &Ingress, invocation: &Invocation) -> Result<Self, Fault> {
        let mut entries = vec![(
            invocation.turn.id,
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

    pub fn current(&self) -> (TurnId, Vec<Reference>, Content) {
        self.entries
            .last()
            .expect("an invocation has input")
            .clone()
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
        let sessions = ingress.sessions(invocation.turn.id);
        let session = sessions
            .get(GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: invocation.turn.session.to_string(),
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
        for (turn, references, content) in &self.entries[..self.entries.len() - 1] {
            if present.contains(&turn.to_string()) {
                continue;
            }
            let mut event = Event::new(invocation.turn.id.to_string());
            event.author = "user".into();
            event.set_content(content.clone());
            ingress
                .sessions(*turn)
                .with_input_source(source)
                .with_references(references.clone())
                .append_event(&invocation.turn.session.to_string(), event)
                .await
                .map_err(model::error)?;
        }
        Ok(())
    }
}
