//! Goose's operation pipeline on the existing execution Node.
mod children;
mod compact;
mod effects;
mod foreground;
pub(crate) mod history;
mod operations;
pub(crate) mod prompt;
mod steer;
#[cfg(test)]
mod tests;
pub(crate) mod tokens;
mod tools;
use crate::{
    agent::{catalog, model},
    store::{
        Ingress,
        agent::{Invocation, RuntimeSession as Session, Sessions},
    },
};
pub(crate) use children::Children;
pub(crate) use effects::Effect;
use goose_agent::{
    events::AgentEvent,
    inference::InferenceRunner,
    machine::{SessionLoader, StateMachine, Step},
    operation::{Emitter, RunStatus},
    subagent::SubagentOperation,
    tool::ToolOperation,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, FaultDetails, ModelFailure, conversation::compaction::Budget,
};
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
    pub callbacks: Vec<Arc<dyn goose_agent::operation::Operation<Session, Effect>>>,
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
    let context = binding
        .provider
        .get_context_limit(&binding.config.model_name, binding.config.context_limit)
        .await;
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
        reported: Default::default(),
    });
    let manual = invocation.turn.kind == sailry_protocol::conversation::RunKind::Compaction;
    let compact = Arc::new(compact::Compaction {
        provider: summary.provider,
        config: summary.config,
        context,
        threshold,
        preparer: preparer.clone(),
        manual,
    });
    let sessions = Sessions::new(ingress.clone(), invocation.turn.id)
        .with_invocation(invocation.clone())
        .with_thinking(binding.config.thinking_effort())
        .with_groupings(catalog.groupings())
        .with_displays(catalog.displays())
        .with_connection_context(bindings.clone(), invocation.connections.resources())
        .with_presentations(catalog.presentations());
    let id = invocation.turn.session.to_string();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let emit = Emitter::new(tx, stop.clone());
    let steps: Vec<Step<'_, Session, Effect>> = if manual {
        vec![Step::Operation(compact)]
    } else {
        let mut steps = vec![
            Step::Operation(Arc::new(operations::PlanReview)),
            Step::Operation(Arc::new(operations::Approval { required, bindings })),
            Step::Operation(Arc::new(prompt::Prompt {
                instruction,
                context: turn_context,
                parts,
            })),
            Step::Operation(Arc::new(tools::Tools {
                inner: ToolOperation::new().with_provider(Arc::new(catalog)),
                controls: ingress.agents.clone(),
                turn: invocation.turn.id,
            })),
            Step::Operation(Arc::new(operations::Remaining)),
            Step::Operation(Arc::new(foreground::Foreground {
                inner: SubagentOperation::new(Arc::new(children.clone())),
                ingress: ingress.clone(),
                turn: invocation.turn.id,
            })),
            Step::Operation(Arc::new(steer::Steer {
                ingress: ingress.clone(),
                invocation: invocation.clone(),
                resources,
                tools: capabilities,
                drained: Mutex::default(),
            })),
            Step::Operation(compact),
            Step::Operation(Arc::new(operations::EmptyResponse)),
            Step::Operation(Arc::new(operations::ExitOnError)),
        ];
        if let Some(limit) = invocation
            .child
            .as_ref()
            .and_then(|child| child.role.as_ref())
            .and_then(|role| role.max_turns)
        {
            steps.push(Step::Operation(Arc::new(operations::MaxTurns(limit))));
        }
        for callback in callbacks {
            steps.push(Step::Operation(callback));
        }
        steps.push(Step::Inference(Arc::new(
            InferenceRunner::new(binding.provider, binding.config).with_request_preparer(preparer),
        )));
        steps
    };
    let machine = StateMachine::new(steps, stop.clone());
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
                let mut result = machine.step(&session, &emit).await?;
                machine
                    .apply(&sessions, &session, &mut result.effects, &emit)
                    .await?;
                if result.status == RunStatus::Continuing
                    || result.applied_step == Some("tool_approval")
                {
                    continue;
                }
                break Ok::<_, anyhow::Error>(result.status);
            }
        };
        tokio::pin!(execution);
        loop {
            tokio::select! {
                result=&mut execution=>break result,
                event=events.recv()=>if let Some(AgentEvent::Message(message))=event
                    && let Err(error)=ingress.partial(invocation.turn.session,invocation.turn.id,message).await {
                        break Err(anyhow::Error::new(error));
                    },
            }
        }
    };
    if result.is_err() {
        stop.cancel();
    }
    // GDK owns cancellation recovery, including interrupted requests and streamed text.
    let finalized = machine.finalize(&sessions, &id, &emit).await;
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
        use goose_provider_types::conversation::message::MessageErrorKind;
        let reason = match message.error_kind() {
            Some(MessageErrorKind::Authentication) => ModelFailure::Unauthorized,
            Some(MessageErrorKind::CreditsExhausted) => ModelFailure::RateLimited,
            _ => ModelFailure::Request,
        };
        let mut failure = Fault::new(
            ErrorCode::Unavailable,
            message
                .content
                .iter()
                .find_map(|content| content.as_error().map(|error| error.message.clone()))
                .unwrap_or_else(|| message.as_concat_text()),
        );
        failure.details = Some(FaultDetails::ModelRequest { reason });
        return Err(failure);
    }
    Ok(output(&session, invocation.turn.id))
}

fn last_error(
    session: &Session,
    turn: sailry_protocol::TurnId,
) -> Option<&goose_provider_types::conversation::message::Message> {
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
        .messages()
        .iter()
        .rev()
        .find(|message| {
            message.role == rmcp::model::Role::Assistant
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
