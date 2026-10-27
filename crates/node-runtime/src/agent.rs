//! Product admission and lifecycle around ADK's only model/tool execution loop.
mod answer;
mod approval;
pub(crate) mod attachments;
mod catalog;
mod compaction;
pub(crate) use compaction::estimate::tokens as estimate_context;
pub(crate) use compaction::policy::resolve as compaction_budget;
pub(crate) mod completion;
mod controls;
mod delegation;
mod diagnostics;
mod guidance;
mod inputs;
mod mcp;
pub(crate) use mcp::check::run as check_mcp;
pub(crate) mod media;
pub(crate) mod model;
pub(crate) mod observation;
pub(crate) use model::plugin::{
    discover as discover_plugin_models, discover_authorized as discover_authorized_models,
};
mod plugins;
mod prompt;
pub(crate) use prompt::session::METADATA as CONTEXT_METADATA;
pub(crate) use prompt::session::compose as compose_context;
mod question;
mod references;
mod review;
mod skills;
mod text;
mod title;
mod tools;

pub(crate) use controls::Controls;

use crate::{
    Error,
    store::{
        Ingress,
        agent::{APP, Invocation, USER},
    },
    tasks::Tasks,
};
use adk_core::RunConfig;
use adk_runner::Runner;
use adk_session::SessionService;
use diagnostics::{Diagnostics, Phase};
use futures::StreamExt;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, SessionId, TurnId, conversation::Status};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

pub(crate) fn start(ingress: Arc<Ingress>, tasks: &Tasks) -> Result<(), Error> {
    let registry = Arc::downgrade(&ingress.model_clients);
    let stopped = ingress.agents.stopped.clone();
    tasks.spawn(async move {
        model::registry::Registry::maintain(registry, stopped).await;
    })?;
    for _ in 0..8 {
        let ingress = ingress.clone();
        let children = tasks.clone();
        tasks.spawn(async move {
            serve(ingress, children).await;
        })?;
    }
    Ok(())
}

async fn serve(ingress: Arc<Ingress>, tasks: Tasks) {
    let controls = &ingress.agents;
    loop {
        let notified = controls.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if controls.stopped.is_cancelled() {
            break;
        }
        let token = controls.stopped.child_token();
        let invocation = match ingress.claim_run(token.clone()).await {
            Ok(Some(invocation)) => invocation,
            Ok(None) => {
                tokio::select! { _ = controls.stopped.cancelled() => break, _ = notified => {} }
                continue;
            }
            Err(error) => {
                let mut entry = crate::logging::Entry::new(
                    crate::logging::Area::Run,
                    crate::logging::Phase::Failed,
                );
                entry.context.node = Some(ingress.node);
                entry.reason = Some("claim");
                entry.code = Some(error.code);
                entry.emit();
                tokio::select! { _ = controls.stopped.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
                continue;
            }
        };
        let _ = run(&ingress, &tasks, invocation, token, None).await;
    }
}

async fn run(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: Invocation,
    token: CancellationToken,
    owner: Option<CancellationToken>,
) -> Result<(Status, Option<String>), Fault> {
    let invocation = Arc::new(invocation);
    let controls = &ingress.agents;
    let turn = invocation.turn.id;
    controls.start_timing(turn);
    let diagnostics = Diagnostics::new(turn);
    let _turn = diagnostics.span(Phase::Turn, None);
    let session = invocation.turn.session;
    let main = invocation.child.is_none();
    let callbacks = plugins::callbacks::capture(&invocation);
    let runner = controls.runner(turn).unwrap_or_else(|| token.child_token());
    let hooks = crate::hooks::Binding::capture(ingress, &invocation, runner.clone());
    let mut completion_review = None;
    let mut result = execute(
        ingress,
        tasks,
        &invocation,
        runner,
        hooks.clone(),
        &diagnostics,
        &mut completion_review,
    )
    .await;
    if let Some(error) = controls.failure(turn)
        && !result
            .as_ref()
            .is_err_and(|error| error.code == ErrorCode::OutcomeUnknown)
    {
        result = Err(error);
    }
    if owner.as_ref().is_some_and(CancellationToken::is_cancelled) {
        // Owner Stop retains pending input before publishing the terminal child run.
        if let Err(error) = ingress.retain_guidance(turn).await {
            result = Err(error);
        }
    }
    if main && (token.is_cancelled() || result.is_err()) {
        // Normal completion leaves background children with their session owner.
        // Interruption retains the existing cancellation and guidance settlement.
        let _settlement = diagnostics.span(Phase::ChildSettlement, None);
        settle_children(ingress, session).await;
    }
    if !controls.stopped.is_cancelled()
        && !token.is_cancelled()
        && let Some(hooks) = &hooks
    {
        let _completion = diagnostics.span(Phase::CompletionHooks, None);
        let status = if result.is_ok() {
            "completed"
        } else {
            "failed"
        };
        if let Err(error) = hooks.complete(status).await {
            merge_failure(&mut result, error);
        }
    }
    // Completion hooks can fail or be interrupted after a successful Runner.
    if main && (token.is_cancelled() || result.is_err()) {
        let _settlement = diagnostics.span(Phase::ChildSettlement, None);
        settle_children(ingress, session).await;
    }
    // Hook and tool process owners survive dropped ADK callback futures.
    {
        let operations = diagnostics.enabled().then(|| controls.operations(turn));
        let _drain = diagnostics.span(Phase::OperationDrain, operations);
        controls.drain(turn).await;
    }
    // ADK can stop polling immediately after a terminal chunk or cancel a stream.
    // Its scope guard has finalized observations before this existing cleanup owner runs.
    for capture in controls.take_observations(turn) {
        if let Err(error) = model::observation::Capture::persist(&capture, ingress, session).await {
            merge_failure(&mut result, model::error(&error));
        }
    }
    if let Some(timeline) = controls.timeline(turn) {
        let mut event = adk_core::Event::new(turn.to_string());
        event.author = "system".into();
        event.provider_metadata.insert(
            "sailry_execution_phases".into(),
            serde_json::to_string(&timeline.summary()).expect("execution phases serialize"),
        );
        if let Err(error) = ingress
            .sessions(turn)
            .append_event(&session.to_string(), event)
            .await
        {
            merge_failure(&mut result, model::error(&error));
        }
    }
    let (status, error, output) = if controls.stopped.is_cancelled() {
        (
            Status::Interrupted,
            Some(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Node stopped execution; calls were not replayed",
            )),
            None,
        )
    } else if token.is_cancelled() {
        (
            Status::Cancelled,
            result
                .err()
                .filter(|error| error.code == ErrorCode::OutcomeUnknown),
            None,
        )
    } else {
        match result {
            Ok(output) => (Status::Completed, None, output),
            Err(error) => (Status::Failed, Some(error), None),
        }
    };
    let persisted = {
        let _persistence = diagnostics.span(Phase::FinishRun, None);
        ingress
            .finish_run_with(
                turn,
                status,
                error.clone().map(plugins::public_failure),
                owner,
                completion_review,
            )
            .await
    };
    if let Err(error) = &persisted {
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Failed);
        entry.context = crate::logging::Context {
            node: Some(ingress.node),
            session: Some(session),
            turn: Some(turn),
            ..Default::default()
        };
        entry.reason = Some("finish");
        entry.code = Some(error.code);
        entry.emit();
    }
    controls.finish(turn);
    if let Ok(status) = &persisted
        && !controls.stopped.is_cancelled()
    {
        plugins::callbacks::complete(ingress, callbacks, *status).await;
    }
    controls.wake.notify_waiters();
    let status = persisted?;
    if let Some(error) = error {
        return Err(error);
    }
    Ok((
        status,
        (status == Status::Completed).then_some(output).flatten(),
    ))
}

async fn settle_children(ingress: &Ingress, session: SessionId) {
    let controls = &ingress.agents;
    controls.cancel_children(session);
    loop {
        let changed = controls.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if !controls.has_children(session) {
            break;
        }
        changed.await;
    }
}

fn merge_failure(result: &mut Result<Option<String>, Fault>, error: Fault) {
    match result {
        Ok(_) => *result = Err(error),
        Err(original) => {
            if error.code == ErrorCode::OutcomeUnknown {
                original.code = ErrorCode::OutcomeUnknown;
            }
            original.message.push_str("; ");
            original.message.push_str(&error.message);
        }
    }
}

async fn execute(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Arc<Invocation>,
    stop: CancellationToken,
    hooks: Option<Arc<crate::hooks::Binding>>,
    diagnostics: &Diagnostics,
    completion_review: &mut Option<adk_core::Event>,
) -> Result<Option<String>, Fault> {
    let session = invocation.turn.session;
    let mut connections = ingress.agents.mcp.take(session);
    let mut result = execute_with(
        ingress,
        tasks,
        invocation,
        stop.clone(),
        &mut connections,
        hooks,
        diagnostics,
    )
    .await
    .map(|outcome| {
        *completion_review = outcome.review;
        outcome.output
    });
    let reusable = result.is_ok() && !stop.is_cancelled();
    let cleanup = if reusable && connections.reusable() {
        let settled = {
            let _settlement = diagnostics.span(Phase::McpSettle, None);
            connections.settle().await
        };
        match settled {
            Ok(()) => {
                let _cache = diagnostics.span(Phase::McpCache, None);
                ingress.agents.mcp.put(session, connections).await
            }
            Err(error) => {
                let _cleanup = diagnostics.span(Phase::McpClose, None);
                match connections.release().await {
                    Ok(()) => Err(error),
                    Err(release) => Err(release),
                }
            }
        }
    } else {
        let _cleanup = diagnostics.span(Phase::McpClose, None);
        connections.close().await
    };
    if let Err(error) = cleanup {
        merge_failure(&mut result, error);
    }
    if result.is_err() || stop.is_cancelled() {
        ingress.model_clients.abandon(session);
    }
    if stop.is_cancelled() {
        let _cleanup = diagnostics.span(Phase::BrowserCleanup, None);
        if let Err(error) = ingress.external_browser.close(session).await {
            merge_failure(
                &mut result,
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    format!("external browser cleanup failed after interruption: {error}"),
                ),
            );
        }
    }
    result
}

#[derive(Default)]
struct Outcome {
    output: Option<String>,
    review: Option<adk_core::Event>,
}

async fn execute_with(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Arc<Invocation>,
    stop: CancellationToken,
    connections: &mut mcp::Connections,
    hooks: Option<Arc<crate::hooks::Binding>>,
    diagnostics: &Diagnostics,
) -> Result<Outcome, Fault> {
    if stop.is_cancelled() {
        return Ok(Outcome::default());
    }
    let purpose = if invocation.turn.kind == sailry_protocol::conversation::RunKind::Compaction {
        model::Purpose::Compaction
    } else {
        model::Purpose::Main
    };
    let model = model::build(ingress, &invocation, &stop, purpose).await?;
    if invocation.turn.kind == sailry_protocol::conversation::RunKind::Compaction {
        compaction::manual(ingress, &invocation, model, stop).await?;
        return Ok(Outcome::default());
    }
    let mut inputs = inputs::Batch::capture(ingress, &invocation).await?;
    let mut prompt_session = prompt::Session::load(ingress, &invocation).await?;
    let resources = Arc::new(crate::plugins::resources::Resources::new(
        ingress.plugins.clone(),
        invocation
            .plugins
            .iter()
            .filter(|package| {
                crate::plugins::conversation::owns_resources(
                    invocation.turn.config.assistant.as_ref(),
                    package,
                )
            })
            .cloned()
            .collect(),
        invocation.plugin_settings.clone(),
        stop.clone(),
    ));
    let capture_output = invocation.child.is_some();
    let planning = invocation.turn.config.mode == sailry_protocol::WorkMode::Plan;
    let reviewing = planning && invocation.child.is_none();
    let metadata = invocation.provider.as_ref().and_then(|provider| {
        provider
            .models
            .iter()
            .find(|model| model.id == invocation.turn.config.model)
    });
    let tool_capable = metadata.is_none_or(|model| model.tools)
        && invocation
            .provider
            .as_ref()
            .and_then(|provider| provider.binding.as_ref())
            .and_then(|binding| binding.adapter.as_ref())
            .is_none_or(|adapter| adapter.capabilities.tools);
    let mut instruction = prompt::baseline().to_owned();
    let workflow = prompt::workflow(
        &invocation.plugins,
        &invocation.turn.config,
        invocation.child.is_some(),
    )?;
    let mut policy = format!(
        "{}\n\n{workflow}",
        prompt::policy(
            invocation.turn.config.mode,
            invocation.turn.config.permission,
            invocation.child.is_some(),
        )
    );
    let personal = prompt::personal(ingress, invocation.worktree, &stop).await?;
    if !personal.is_empty() {
        ingress.agents.context_source(
            invocation.turn.id,
            sailry_protocol::conversation::observation::CategoryKind::Injected,
            &personal,
        );
    }
    if let Some(role) = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
    {
        policy.push_str("\n\n");
        policy.push_str(&role.instructions);
        for skill in &role.skills {
            let text = resources.read(skill, "SKILL.md").await?;
            ingress.agents.context_source(
                invocation.turn.id,
                sailry_protocol::conversation::observation::CategoryKind::Skills,
                &text.content,
            );
            policy.push_str(&format!(
                "\n\nSelected skill {skill}\nDirectory: {}\n{}",
                text.directory, text.content
            ));
        }
    }
    let mut extensions = plugins::bind(
        ingress,
        tasks,
        &invocation,
        prompt_session.loaded(),
        &stop,
        resources.clone(),
        connections,
    )
    .await?;
    let completion = extensions.completion.clone();
    let environment = prompt::environment(ingress, &invocation, &stop).await;
    for captured in &invocation.instructions {
        if tool_capable
            && captured
                .any_tools
                .iter()
                .any(|name| extensions.mounted(name))
        {
            policy.push_str("\n\n");
            policy.push_str(&captured.text);
            ingress.agents.context_source(
                invocation.turn.id,
                sailry_protocol::conversation::observation::CategoryKind::Injected,
                &captured.text,
            );
        }
    }
    if !extensions.instruction.is_empty() {
        policy.push_str("\n\n");
        policy.push_str(&extensions.instruction);
        ingress.agents.context_source(
            invocation.turn.id,
            sailry_protocol::conversation::observation::CategoryKind::Injected,
            &extensions.instruction,
        );
    }
    instruction.push_str("\n\n");
    instruction.push_str(prompt::recipient(invocation.child.is_some()));
    let capability_tools = references::capabilities::tools(&extensions.tools);
    inputs
        .select(&invocation, &resources, &capability_tools)
        .await?;
    let (input_turn, input_references, content) = inputs.current();
    if let Some(title) = title::instruction(
        invocation.title.as_deref(),
        tool_capable,
        invocation.child.is_some(),
    ) {
        policy.push_str("\n\n");
        policy.push_str(&title);
    }
    let instruction = prompt_session.instruction(instruction);
    let (compact, compact_tool, context) = compaction::bind(
        ingress,
        model::build(ingress, &invocation, &stop, model::Purpose::Compaction).await?,
        &invocation,
        stop.clone(),
        instruction.clone(),
    );
    if tool_capable {
        extensions
            .tools
            .push(catalog::Registration::managed(compact_tool));
        if invocation.child.is_none() {
            extensions.tools.extend([
                title::bind(invocation.turn.session.to_string(), stop.clone()),
                catalog::Registration::managed(Arc::new(question::Question {
                    ingress: ingress.clone(),
                    turn: invocation.turn.id,
                    session: invocation.turn.session.to_string(),
                    stop: stop.clone(),
                    planning: reviewing,
                })),
            ]);
        }
    }
    let declarations = prompt_session.declarations(&extensions.tools);
    let preflight_tools = prompt::session::project(declarations.clone());
    prompt_session
        .append(ingress, &invocation, policy, environment, personal, &stop)
        .await?;
    let preamble = prompt::bind(instruction.clone(), declarations);
    let mut catalog = catalog::Catalog::new(
        invocation.turn.session.to_string(),
        stop.clone(),
        ingress.agents.timeline(invocation.turn.id),
    );
    let bindings = approval::bindings(&invocation, &extensions.tools)?;
    let limit = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
        .and_then(|role| role.max_turns);
    let (guidance, continuation) = guidance::bind(
        ingress,
        invocation,
        context.clone(),
        resources,
        capability_tools,
        stop.clone(),
    );
    let mut agent = adk_agent::LlmAgentBuilder::new("assistant")
        // ADK requires a numeric ceiling; only explicit role budgets constrain tasks.
        .max_iterations(limit.unwrap_or(u32::MAX))
        .model(context.model(model.clone()))
        .expose_tool_error_messages(true)
        .on_tool_error(plugins::failure_callback())
        .instruction_provider(Box::new(move |_| {
            let instruction = instruction.clone();
            Box::pin(async move { Ok(instruction) })
        }))
        .after_model_callback(continuation)
        .before_model_callback(approval::bind(ingress, invocation.turn.id, content.clone()))
        .before_model_callback(preamble)
        .before_model_callback(guidance);
    if let Some(hooks) = hooks {
        agent = agent
            .before_tool_callback(hooks.before())
            .after_tool_callback_full(hooks.after());
    }
    extensions.callbacks.push((0, compact));
    extensions.callbacks.sort_by_key(|(order, _)| *order);
    for (_, callback) in extensions.callbacks {
        agent = agent.before_model_callback(callback);
    }
    if let Some(model) = metadata {
        agent = agent.max_output_tokens(model.output as i32);
    }
    if tool_capable {
        agent = agent
            .parallelize_agent_delegations(true)
            .tool_execution_strategy(adk_core::ToolExecutionStrategy::Auto);
    }
    for registration in extensions.tools {
        let confirmation = !registration.managed && !registration.tool.is_read_only();
        let tool = catalog.register(registration)?;
        if confirmation {
            agent = agent.require_tool_confirmation(tool.name());
        }
        agent = agent.tool(tool);
    }
    let agent = agent.build().map_err(model::error)?;
    let sessions = Arc::new(
        crate::store::agent::Sessions::new(ingress.clone(), invocation.turn.id)
            .with_input_turn(input_turn)
            .with_input_source(if invocation.child.is_some() {
                "parent"
            } else if invocation.notice {
                "notice"
            } else if invocation.automatic {
                "plugin"
            } else {
                "user"
            })
            .with_notices(invocation.notices.clone(), invocation.notice_input.clone())
            .with_references(input_references)
            .with_groupings(catalog.groupings())
            .with_displays(catalog.displays())
            .with_connection_context(bindings.clone(), invocation.connections.resources())
            .with_presentations(catalog.presentations()),
    );
    let run_config = RunConfig {
        // The Node's storage actor owns expiry and durable approval in one order.
        tool_confirmation_timeout: None,
        tool_confirmation_handler: Some(Arc::new(approval::Handler {
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            stop: stop.clone(),
            bindings,
        })),
        ..Default::default()
    };
    let runner = Runner::builder()
        .app_name(APP)
        .agent(Arc::new(agent))
        .session_service(sessions.clone())
        .run_config(run_config);
    let runner = runner.build().map_err(model::error)?;
    let session = invocation.turn.session.to_string();
    let target = invocation.turn.session;
    let turn = invocation.turn.id;
    let user = adk_core::UserId::try_from(USER)
        .map_err(|error| model::error(adk_core::AdkError::from(error)))?;
    let session_id = adk_core::SessionId::try_from(session.as_str())
        .map_err(|error| model::error(adk_core::AdkError::from(error)))?;
    // Tool discovery can be interrupted before ADK owns an invocation.
    if stop.is_cancelled() {
        return Ok(Outcome::default());
    }
    context
        .preflight(&inputs.pending(), preflight_tools)
        .await
        .map_err(model::error)?;
    if stop.is_cancelled() {
        return Ok(Outcome::default());
    }
    inputs
        .append(
            ingress,
            &invocation,
            if invocation.child.is_some() {
                "parent"
            } else {
                "user"
            },
        )
        .await?;
    let mut runner_span = diagnostics.span(Phase::Runner, None);
    let mut events = runner
        .run(user, session_id, content)
        .await
        .map_err(model::error)?;
    let mut cancelled = false;
    let mut output = None;
    let mut has_result = false;
    let mut proposal = None;
    let mut requested_review = false;
    loop {
        let next = tokio::select! {
            biased;
            _ = stop.cancelled(), if !cancelled => {
                // ADK owns cancellation and its invocation context; there is no second loop.
                runner.interrupt_identity(APP, USER, &session);
                cancelled = true;
                continue;
            }
            next = events.next() => next,
        };
        match next {
            Some(Ok(event)) => {
                if diagnostics.enabled() && event.content().is_some() {
                    runner_span.content();
                }
                // Runner has already committed non-partial events through Sessions.
                if event.llm_response.error_code.is_some()
                    || event.llm_response.error_message.is_some()
                {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        event
                            .llm_response
                            .error_message
                            .clone()
                            .unwrap_or_else(|| "model response reported an error".into()),
                    ));
                }
                if event.llm_response.interrupted && !stop.is_cancelled() {
                    return Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "model response was interrupted",
                    ));
                }
                answer::validate(&event)?;
                has_result |= answer::present(&event) || (reviewing && answer::reviewed(&event));
                if reviewing {
                    requested_review |= review::requested(&event);
                    if answer::present(&event) {
                        proposal = Some(event.id.clone());
                    }
                }
                if event.llm_response.partial {
                    if let Some(started) = durable_tool_start(&event) {
                        sessions
                            .append_event(&session, started)
                            .await
                            .map_err(model::error)?;
                    } else {
                        ingress.partial(target, turn, event).await?;
                    }
                } else if capture_output && let Some(content) = event.content() {
                    let text = content
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            adk_core::Part::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if content.role == "model" && !text.is_empty() {
                        output = Some(text);
                    }
                }
            }
            Some(Err(error)) => return Err(model::error(error)),
            None => {
                drop(runner_span);
                if !cancelled
                    && !stop.is_cancelled()
                    && !has_result
                    && !completion.load(Ordering::Acquire)
                {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "Model returned no answer",
                    ));
                }
                let review = if reviewing
                    && !requested_review
                    && !cancelled
                    && !stop.is_cancelled()
                    && let Some(source) = proposal
                {
                    let _review = diagnostics.span(Phase::PlanReview, None);
                    Some(review::completion(turn, source))
                } else {
                    None
                };
                return Ok(Outcome { output, review });
            }
        }
    }
}

fn durable_tool_start(event: &adk_core::Event) -> Option<adk_core::Event> {
    let metadata = event.provider_metadata.get("adk_tool_started")?;
    let metadata = serde_json::from_str::<serde_json::Value>(metadata).ok()?;
    let fields = metadata.as_object()?;
    let id = fields.get("id")?;
    let name = fields.get("name")?.as_str()?;
    if !event.llm_response.partial
        || fields.len() != 2
        || !(id.is_null() || id.is_string())
        || name.is_empty()
        || event.provider_metadata.len() != 1
        || event.content().is_some()
        || event.llm_response.usage_metadata.is_some()
        || event.llm_response.finish_reason.is_some()
        || event.llm_response.citation_metadata.is_some()
        || event.llm_response.turn_complete
        || event.llm_response.interrupted
        || event.llm_response.error_code.is_some()
        || event.llm_response.error_message.is_some()
        || event.llm_response.provider_metadata.is_some()
        || event.llm_response.interaction_id.is_some()
        || !event.actions.state_delta.is_empty()
        || !event.actions.artifact_delta.is_empty()
        || event.actions.skip_summarization
        || event.actions.transfer_to_agent.is_some()
        || event.actions.escalate
        || event.actions.tool_confirmation.is_some()
        || event.actions.tool_confirmation_decision.is_some()
        || event.actions.compaction.is_some()
        || event.actions.route.is_some()
        || !event.long_running_tool_ids.is_empty()
        || event.llm_request.is_some()
    {
        return None;
    }
    let mut started = event.clone();
    started.llm_response.partial = false;
    Some(started)
}

#[cfg(test)]
mod tool_starts {
    use super::*;

    fn event(id: serde_json::Value) -> adk_core::Event {
        let mut event = adk_core::Event::new("invocation");
        event.author = "assistant".into();
        event.branch = "branch".into();
        event.llm_response.partial = true;
        event.provider_metadata.insert(
            "adk_tool_started".into(),
            serde_json::json!({"id": id, "name": "ask_user"}).to_string(),
        );
        event
    }

    #[test]
    fn normalizes_nullable_id() {
        for id in [serde_json::Value::Null, serde_json::json!("call")] {
            let event = event(id);
            let started = durable_tool_start(&event).unwrap();
            let mut expected = event.clone();
            expected.llm_response.partial = false;
            assert!(event.llm_response.partial);
            assert_eq!(
                serde_json::to_value(started).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }

    #[test]
    fn rejects_malformed_metadata() {
        for metadata in [
            "not-json".into(),
            serde_json::json!({"name": "ask_user"}).to_string(),
            serde_json::json!({"id": 1, "name": "ask_user"}).to_string(),
            serde_json::json!({"id": null, "name": ""}).to_string(),
            serde_json::json!({"id": null, "name": "ask_user", "extra": true}).to_string(),
        ] {
            let mut event = event(serde_json::Value::Null);
            event
                .provider_metadata
                .insert("adk_tool_started".into(), metadata);
            assert!(durable_tool_start(&event).is_none());
        }
    }

    #[test]
    fn rejects_lifecycle_payloads() {
        let plain = event(serde_json::Value::Null);
        let mut cases = Vec::new();
        let mut content = plain.clone();
        content.set_content(adk_core::Content::new("model").with_text("unexpected"));
        cases.push(content);
        let mut completed = plain.clone();
        completed.llm_response.turn_complete = true;
        cases.push(completed);
        let mut state = plain.clone();
        state
            .actions
            .state_delta
            .insert("unexpected".into(), serde_json::json!(true));
        cases.push(state);
        let mut metadata = plain;
        metadata
            .provider_metadata
            .insert("unexpected".into(), "value".into());
        cases.push(metadata);
        assert!(
            cases
                .iter()
                .all(|event| durable_tool_start(event).is_none())
        );
    }

    #[test]
    fn preserves_other_partials() {
        let mut event = adk_core::Event::new("invocation");
        event.llm_response.partial = true;
        event.set_content(adk_core::Content::new("model").with_text("draft"));
        assert!(durable_tool_start(&event).is_none());
        assert!(event.llm_response.partial);
    }
}

#[cfg(test)]
mod failures {
    use super::*;

    #[test]
    fn preserves_original_cause() {
        let mut result = Err(Fault::new(ErrorCode::PermissionDenied, "original failure"));
        merge_failure(
            &mut result,
            Fault::new(ErrorCode::Unavailable, "hook failure"),
        );
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(error.message, "original failure; hook failure");
    }

    #[test]
    fn retains_uncertainty_from_either_cause() {
        for (original, hook) in [
            (ErrorCode::OutcomeUnknown, ErrorCode::Unavailable),
            (ErrorCode::Unavailable, ErrorCode::OutcomeUnknown),
        ] {
            let mut result = Err(Fault::new(original, "original failure"));
            merge_failure(&mut result, Fault::new(hook, "hook failure"));
            let error = result.unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
            assert_eq!(error.message, "original failure; hook failure");
        }
    }

    #[test]
    fn promotes_failure_after_success() {
        let error = Fault::new(ErrorCode::Unavailable, "hook failure");
        let mut result = Ok(Some("original answer".into()));
        merge_failure(&mut result, error.clone());
        assert_eq!(result, Err(error));
    }
}
