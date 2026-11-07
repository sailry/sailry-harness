//! The Node-owned execution loop and canonical effect boundary.
//! Ordering derives from Goose 14741e9a machine.rs (Apache-2.0).
//! Plan review, approval, tools, remaining calls, foreground children, steer,
//! compaction, empty-response reveal, error exit, turn limit, callbacks, inference.
//! The first applicable stage saves its effects before the next reload. Prompt
//! contributions are collected for inference; manual compaction runs alone.
//! See third_party_licenses/goose-Apache-2.0.txt.
mod children;
mod compact;
pub(crate) mod conversation;
mod effects;
mod foreground;
pub(crate) mod history;
mod inference;
pub(crate) mod operation;
mod operations;
pub(crate) mod prompt;
mod steer;
#[cfg(test)]
mod tests;
pub(crate) mod tokens;
pub(super) mod tools;
use crate::{
    agent::{catalog, model},
    store::{
        Ingress,
        agent::{Invocation, RuntimeSession as Session, Sessions},
    },
};
pub(crate) use children::Children;
pub(crate) use effects::{ConversationEffect, Effect};
pub(crate) use operation::{Callback, Emitter, Outcome, RunStatus};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, conversation::compaction::Budget};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

/// One trigger for the reported budget and the execution threshold.
pub(crate) fn trigger(context: u64, configured: Option<u32>, percent: u8) -> u64 {
    configured
        .map(u64::from)
        .unwrap_or(context * u64::from(percent) / 100)
}

pub(crate) fn budget(
    model: &str,
    context: u32,
    output: u32,
    configured: Option<u32>,
    percent: u8,
) -> Option<Budget> {
    if context == 0 {
        return None;
    }
    Some(Budget {
        model: model.into(),
        context_limit: context,
        trigger_tokens: trigger(u64::from(context), configured, percent),
        output_reserve: output,
        recent_tokens: 0,
        summary_input: u64::from(context.saturating_sub(model::summary::output_limit(output))),
        summary_output: model::summary::output_limit(output),
    })
}

pub(super) struct Run<'a> {
    pub ingress: &'a Arc<Ingress>,
    pub invocation: &'a Arc<Invocation>,
    pub stop: CancellationToken,
    pub binding: model::Binding,
    pub summary: model::Binding,
    pub instruction: String,
    pub context: String,
    /// Plugin parts persisted once per conversation, after the turn context.
    pub parts: Vec<String>,
    pub catalog: catalog::Catalog,
    pub callbacks: Vec<Arc<dyn Callback>>,
    pub images: Vec<(Vec<String>, usize)>,
    pub required: BTreeSet<String>,
    pub bindings: BTreeMap<String, crate::store::agent::ApprovalBinding>,
    pub deferred: BTreeSet<String>,
    pub resources: Arc<crate::plugins::resources::Resources>,
    pub capabilities: BTreeMap<String, Vec<String>>,
    pub children: Children,
}

pub(super) async fn run(run: Run<'_>) -> Result<Option<String>, Fault> {
    let Run {
        ingress,
        invocation,
        stop,
        binding,
        summary,
        instruction,
        context: turn_context,
        parts,
        catalog,
        callbacks,
        images,
        required,
        bindings,
        deferred,
        resources,
        capabilities,
        children,
    } = run;
    let startup = crate::agent::diagnostics::Startup::new(ingress.node, invocation, &stop);
    let setup = std::time::Instant::now();
    let context = model::context_limit(ingress, invocation, &stop).await;
    startup.record(startup.entry("context_limit", setup, None));
    let configured = invocation
        .provider
        .as_ref()
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.id == invocation.turn.config.model)
        })
        .and_then(|model| model.compaction_tokens);
    let threshold =
        trigger(context as u64, configured, invocation.compaction_percent) as f64 / context as f64;
    let preparer = Arc::new(prompt::Preparer {
        ingress: ingress.clone(),
        invocation: invocation.clone(),
        stop: stop.clone(),
        deferred,
        images,
        parts: parts.clone(),
        sources: binding.sources.clone(),
        attachments: Default::default(),
        output: Default::default(),
    });
    let manual = invocation.turn.kind == sailry_protocol::conversation::RunKind::Compaction;
    let compact = compact::Compaction {
        provider: summary.provider,
        config: summary.config,
        context,
        threshold,
        preparer: preparer.clone(),
        manual,
    };
    let sessions = Sessions::new(ingress.clone(), invocation.turn.id)
        .with_invocation(invocation.clone())
        .with_thinking(Some(binding.config.thinking_effort))
        .with_groupings(catalog.groupings())
        .with_displays(catalog.displays())
        .with_delegations(catalog.delegations())
        .with_connection_context(bindings.clone(), invocation.connections.resources())
        .with_presentations(catalog.presentations());
    let id = invocation.turn.session.to_string();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let emit = Emitter::new(tx, stop.clone());
    let engine = Engine {
        manual,
        compact,
        approval: operations::Approval { required, bindings },
        prompt: prompt::Prompt {
            instruction,
            context: turn_context,
            parts,
        },
        tools: tools::Tools::new(
            Arc::new(catalog),
            ingress.agents.clone(),
            invocation.turn.id,
        ),
        foreground: foreground::Foreground::new(children, ingress.clone(), invocation.turn.id),
        steer: steer::Steer {
            ingress: ingress.clone(),
            invocation: invocation.clone(),
            resources,
            tools: capabilities,
            attachments: preparer.attachments.clone(),
            drained: Mutex::default(),
        },
        max_turns: invocation
            .child
            .as_ref()
            .and_then(|child| child.role.as_ref())
            .and_then(|role| role.max_turns)
            .map(operations::MaxTurns),
        callbacks,
        inference: inference::Inference::new(binding, preparer),
    };
    let mut first = true;
    let result = {
        let execution = async {
            loop {
                let loading = std::time::Instant::now();
                let session = sessions.load(&id).await;
                if std::mem::take(&mut first) {
                    let code = session.as_ref().err().map(|_| ErrorCode::Unavailable);
                    startup.record(startup.entry("engine_session_load", loading, code));
                    startup.record(startup.entry("engine_setup", setup, code));
                }
                let session = session?;
                let step = advance(engine.step(&session, &emit), &stop).await?;
                let (mut transition, approval) = step;
                save(&sessions, &session, &mut transition.effects).await?;
                if transition.status == RunStatus::Continuing || approval {
                    continue;
                }
                break Ok::<_, anyhow::Error>(transition.status);
            }
        };
        tokio::pin!(execution);
        loop {
            tokio::select! {
                result=&mut execution=>break result,
                event=events.recv()=>if let Some(message)=event
                    && let Err(error)=ingress.partial(invocation.turn.session,invocation.turn.id,message).await {
                        break Err(anyhow::Error::new(error));
                    },
            }
        }
    };
    if result.is_err() {
        stop.cancel();
    }
    let finalized = if stop.is_cancelled() {
        engine.finalize(&sessions, &id).await
    } else {
        Ok(())
    };
    result.map_err(fault)?;
    finalized.map_err(fault)?;
    let session = sessions.load(&id).await.map_err(fault)?;
    if let Some(message) = last_error(&session, invocation.turn.id) {
        if let Some(provider) = &invocation.provider
            && let Some(failure) = ingress
                .authorization_failure(provider, invocation.turn.config.credential.as_ref())
                .await?
        {
            return Err(failure);
        }
        return Err(model::error(model::Error::new(
            message.error_kind().unwrap_or(model::ErrorKind::Other),
            message
                .content
                .iter()
                .find_map(|content| match content {
                    model::Content::Error { message, .. } => Some(message.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| message.as_concat_text()),
        )));
    }
    Ok(output(&session, invocation.turn.id))
}

async fn advance(
    work: impl std::future::Future<Output = anyhow::Result<(operation::Transition, bool)>>,
    stop: &CancellationToken,
) -> anyhow::Result<(operation::Transition, bool)> {
    let result = tokio::select! {
        biased;
        result = work, if !stop.is_cancelled() => Some(result),
        _ = stop.cancelled() => None,
    };
    match result {
        Some(Ok(transition)) => Ok(transition),
        Some(Err(error)) if !stop.is_cancelled() => Err(error),
        _ => Ok((
            operation::Transition {
                effects: Vec::new(),
                status: RunStatus::Cancelled,
            },
            false,
        )),
    }
}

struct Engine {
    manual: bool,
    compact: compact::Compaction,
    approval: operations::Approval,
    prompt: prompt::Prompt,
    tools: tools::Tools,
    foreground: foreground::Foreground,
    steer: steer::Steer,
    max_turns: Option<operations::MaxTurns>,
    callbacks: Vec<Arc<dyn Callback>>,
    inference: inference::Inference,
}

impl Engine {
    async fn step(
        &self,
        session: &Session,
        emit: &Emitter,
    ) -> anyhow::Result<(operation::Transition, bool)> {
        let conversation = &session.conversation;
        macro_rules! stage {
            ($call:expr, $approval:expr) => {
                if let Outcome::Applied(transition) = $call.await? {
                    return Ok((transition, $approval));
                }
            };
        }
        if self.manual {
            stage!(self.compact.run(session, conversation, emit), false);
        } else {
            stage!(
                operations::PlanReview.run(session, conversation, emit),
                false
            );
            stage!(self.approval.run(session, conversation, emit), true);
            stage!(self.tools.run(session, conversation, emit), false);
            stage!(
                operations::Remaining.run(session, conversation, emit),
                false
            );
            stage!(self.foreground.run(session, conversation, emit), false);
            stage!(self.steer.run(session, conversation, emit), false);
            stage!(self.compact.run(session, conversation, emit), false);
            stage!(
                operations::EmptyResponse.run(session, conversation, emit),
                false
            );
            stage!(
                operations::ExitOnError.run(session, conversation, emit),
                false
            );
            if let Some(limit) = &self.max_turns {
                stage!(limit.run(session, conversation, emit), false);
            }
            for callback in &self.callbacks {
                stage!(callback.run(session, conversation, emit), false);
            }
            let mut input = prompt::InferenceInput {
                tools: self.tools.definitions(session).await?,
                instructions: self.prompt.instructions(session).await?,
                contexts: self.prompt.contexts(session).await?,
            };
            if let Some(limit) = &self.max_turns {
                input.contexts.extend(limit.contexts(conversation).await?);
            }
            stage!(self.inference.run(session, input, emit), false);
        }
        Ok((
            operation::Transition {
                effects: Vec::new(),
                status: RunStatus::Yielded,
            },
            false,
        ))
    }

    async fn finalize(&self, sessions: &Sessions, id: &str) -> anyhow::Result<()> {
        let mut session = sessions.load(id).await?;
        let mut effects = self.tools.finalize_cancellation();
        effects.extend(self.inference.finalize_cancellation());
        effects.extend(self.steer.finalize_cancellation());
        if !effects.is_empty() {
            save(sessions, &session, &mut effects).await?;
            session = sessions.load(id).await?;
        }
        let mut interrupted = model::Message::user();
        let mut answered = BTreeSet::new();
        for call in conversation::pending(&session.conversation).unwrap_or_default() {
            if answered.insert(call.id.as_str()) {
                interrupted.add_tool_response_with_metadata(
                    &call.id,
                    tools::interrupted_call(),
                    call.metadata.as_ref(),
                );
            }
        }
        if !interrupted.content.is_empty() {
            save(sessions, &session, &mut [interrupted.into()]).await?;
            session = sessions.load(id).await?;
        }
        let mut effects = self.foreground.finalize_cancellation(&session).await;
        if !effects.is_empty() {
            save(sessions, &session, &mut effects).await?;
        }
        Ok(())
    }
}

async fn save(
    sessions: &Sessions,
    session: &Session,
    effects: &mut [Effect],
) -> anyhow::Result<()> {
    for effect in effects.iter_mut() {
        effect.identify();
    }
    sessions.apply_effects(session, effects).await
}

fn last_error(session: &Session, turn: sailry_protocol::TurnId) -> Option<&model::Message> {
    session.conversation.last().filter(|message| {
        message.error_kind().is_some()
            && session.records.iter().any(|record| {
                record.turn == turn && message.id.as_deref() == Some(record.id.as_str())
            })
    })
}

fn output(session: &Session, turn: sailry_protocol::TurnId) -> Option<String> {
    session
        .conversation
        .iter()
        .rev()
        .find(|message| {
            message.role == model::Role::Assistant
                && message.is_user_visible()
                && !message.as_concat_text().is_empty()
                && session.records.iter().any(|record| {
                    record.turn == turn && message.id.as_deref() == Some(record.id.as_str())
                })
        })
        .map(|message| message.as_concat_text())
}

pub(super) fn fault(error: anyhow::Error) -> Fault {
    error
        .downcast_ref::<Fault>()
        .cloned()
        .unwrap_or_else(|| Fault::new(ErrorCode::Unavailable, error.to_string()))
}
