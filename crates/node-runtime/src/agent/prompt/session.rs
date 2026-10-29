//! Captured context is typed Node state, injected by the prompt operation.
use super::instructions::{self, Scope};
use crate::{
    agent::canonical::{Context, Guide, Kind, Record},
    store::{
        Ingress,
        agent::{Invocation, SessionQuery, Sessions},
    },
};
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, conversation::reference::Target};
use std::{collections::BTreeSet, sync::Arc};

pub(in crate::agent) struct Session {
    context: Context,
    sessions: Sessions,
    pub tool_names: BTreeSet<String>,
}

impl Session {
    pub async fn load(ingress: &Arc<Ingress>, invocation: &Invocation) -> Result<Self, Fault> {
        let sessions = Sessions::new(ingress.clone(), invocation.turn.id);
        let session = sessions
            .get(SessionQuery {
                session: invocation.turn.session,
                recent: Some(0),
                after: None,
            })
            .await?;
        Ok(Self {
            context: session.context,
            sessions,
            tool_names: session.tool_names,
        })
    }
    pub fn loaded(&self) -> BTreeSet<String> {
        self.context.loaded_tools.clone().unwrap_or_default()
    }

    pub async fn append(
        &self,
        ingress: &Ingress,
        invocation: &Invocation,
        policy: String,
        environment: String,
        personal: String,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let scopes: Vec<_> = invocation
            .message
            .references
            .iter()
            .chain(
                invocation
                    .followups
                    .iter()
                    .flat_map(|input| &input.message.references),
            )
            .filter_map(|reference| match &reference.target {
                Target::File(path) => Some(Scope::File(path.clone())),
                Target::Directory(path) => Some(Scope::Directory(path.clone())),
                _ => None,
            })
            .collect();
        let guides = if invocation.project.is_some() && invocation.connections.bound.is_none() {
            instructions::capture(ingress, invocation.worktree, &scopes, stop).await?
        } else {
            Vec::new()
        };
        let mut delta = Context {
            captured: true,
            ..Context::default()
        };
        if self.context.policy.as_ref() != Some(&policy) {
            delta.policy = Some(policy);
        }
        if self.context.environment.as_ref() != Some(&environment) {
            delta.environment = Some(environment);
        }
        if self.context.personal.as_ref() != Some(&personal) {
            delta.personal = Some(personal);
        }
        for guide in guides {
            let value = Guide {
                path: guide.path.clone(),
                revision: guide.revision.clone(),
                content: guide.render(),
            };
            if self
                .context
                .guides
                .get(&guide.key())
                .is_none_or(|previous| previous.revision != value.revision)
            {
                delta.guides.insert(guide.key(), value);
            }
        }
        if delta.policy.is_none()
            && delta.environment.is_none()
            && delta.personal.is_none()
            && delta.guides.is_empty()
        {
            return Ok(());
        }
        let mut record = Record::new(
            invocation.turn.id,
            Kind::Configuration(crate::agent::canonical::Configuration { context: delta }),
        );
        record.provenance.author = "system".into();
        self.sessions
            .append_event(&invocation.turn.session.to_string(), record)
            .await
    }
}

#[cfg(test)]
mod tests;
