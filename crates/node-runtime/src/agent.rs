//! Product admission and lifecycle around Goose GDK's execution pipeline.
mod approval;
pub(crate) mod attachments;
pub(crate) mod canonical;
pub(crate) mod catalog;
pub(crate) mod engine;
pub(crate) use engine::budget as compaction_budget;
pub(crate) use engine::tokens::records as estimate_context;
pub(crate) mod completion;
mod controls;
mod delegation;
mod diagnostics;
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

mod question;
mod references;
mod review;
mod settlement;
mod skills;
mod title;
mod tools;

pub(crate) use controls::Controls;

use crate::{
    Error,
    store::{Ingress, agent::Invocation},
    tasks::Tasks,
};
use diagnostics::{Diagnostics, Phase};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, SessionId, TurnId, conversation::Status};
use std::{sync::Arc, time::Duration};

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
    if let Some(timeline) = controls.timeline(turn) {
        let mut record = canonical::Record::new(
            turn,
            canonical::Kind::Message(canonical::Message::default()),
        );
        record.provenance.author = "system".into();
        record.metadata.execution = Some(canonical::Execution {
            phases: Some(timeline.summary()),
            ..Default::default()
        });
        record.metadata.runtime = Some(canonical::Runtime {
            user_visible: false,
            agent_visible: false,
            ..Default::default()
        });
        if let Err(error) = ingress
            .sessions(turn)
            .append_event(&session.to_string(), record)
            .await
        {
            merge_failure(&mut result, error);
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
        settlement::finish(
            ingress,
            turn,
            session,
            status,
            error.clone().map(plugins::public_failure),
            owner,
            completion_review,
        )
        .await
    };
    if persisted.is_ok() {
        controls.finish(turn);
    }
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
    completion_review: &mut Option<canonical::Record>,
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
    review: Option<canonical::Record>,
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
    let model = model::provider(ingress, invocation, &stop, purpose).await?;
    let prompt_session = prompt::Session::load(ingress, invocation).await?;
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
    if let Some(role) = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
    {
        policy.push_str("\n\n");
        policy.push_str(&role.instructions);
        for skill in &role.skills {
            let text = resources.read(skill, "SKILL.md").await?;
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
        }
    }
    if !extensions.instruction.is_empty() {
        policy.push_str("\n\n");
        policy.push_str(&extensions.instruction);
    }
    instruction.push_str("\n\n");
    instruction.push_str(prompt::recipient(invocation.child.is_some()));
    if let Some(title) = title::instruction(
        invocation.title.as_deref(),
        tool_capable,
        invocation.child.is_some(),
    ) {
        policy.push_str("\n\n");
        policy.push_str(&title);
    }
    if tool_capable && invocation.child.is_none() {
        extensions.tools.push(title::bind(
            invocation.turn.session.to_string(),
            stop.clone(),
        ));
        extensions.tools.push(
            question::Question {
                ingress: ingress.clone(),
                turn: invocation.turn.id,
                session: invocation.turn.session.to_string(),
                stop: stop.clone(),
                planning: reviewing,
            }
            .registration(),
        );
    }
    let capability_tools = references::capabilities::tools(&extensions.tools);
    if invocation.turn.kind != sailry_protocol::conversation::RunKind::Compaction {
        let mut inputs = inputs::Batch::capture(ingress, invocation).await?;
        inputs
            .select(invocation, &resources, &capability_tools)
            .await?;
        prompt_session
            .append(ingress, invocation, policy, environment, personal, &stop)
            .await?;
        inputs
            .append(
                ingress,
                invocation.turn.session,
                invocation.turn.id,
                if invocation.child.is_some() {
                    "parent"
                } else if invocation.notice {
                    "notice"
                } else if invocation.automatic {
                    "plugin"
                } else {
                    "user"
                },
                false,
            )
            .await?;
    }
    if stop.is_cancelled() {
        return Ok(Outcome::default());
    }
    let children = engine::Children::new(ingress.clone(), tasks.clone(), invocation);
    let mut catalog = catalog::Catalog::new(
        invocation.turn.session.to_string(),
        stop.clone(),
        ingress.agents.timeline(invocation.turn.id),
    )
    .with_children(children.clone())
    .with_hooks(hooks);
    let bindings = approval::bindings(invocation, &extensions.tools)?;
    let required = extensions
        .tools
        .iter()
        .filter(|entry| !entry.managed && !entry.read_only())
        .map(|entry| entry.tool.name.to_string())
        .collect();
    let deferred = extensions
        .tools
        .iter()
        .filter(|entry| entry.deferred)
        .map(|entry| entry.tool.name.to_string())
        .collect();
    for registration in extensions.tools {
        catalog.register(registration)?;
    }
    extensions.callbacks.sort_by_key(|(order, _)| *order);
    let callbacks = extensions
        .callbacks
        .into_iter()
        .map(|(_, callback)| {
            callback
                as Arc<
                    dyn goose_agent::operation::Operation<
                            crate::store::agent::RuntimeSession,
                            engine::Effect,
                        >,
                >
        })
        .collect();
    let summary = model::provider(ingress, invocation, &stop, model::Purpose::Compaction).await?;
    let _runner = diagnostics.span(Phase::Runner, None);
    let output = engine::run(
        ingress,
        invocation,
        stop.clone(),
        model,
        summary,
        instruction,
        catalog,
        callbacks,
        extensions.images,
        required,
        bindings,
        deferred,
        resources,
        capability_tools,
        children,
    )
    .await?;
    let review = if reviewing && !stop.is_cancelled() {
        let session = ingress
            .sessions(invocation.turn.id)
            .get(crate::store::agent::SessionQuery {
                session: invocation.turn.session,
                recent: None,
                after: None,
            })
            .await?;
        let requested = session
            .conversation
            .messages()
            .iter()
            .any(review::requested);
        if requested {
            None
        } else {
            session
                .conversation
                .messages()
                .iter()
                .rev()
                .find(|message| {
                    message.role == rmcp::model::Role::Assistant
                        && message.is_user_visible()
                        && !message.as_concat_text().is_empty()
                })
                .and_then(|message| message.id.clone())
                .map(|source| review::completion(invocation.turn.id, source))
        }
    } else {
        None
    };
    Ok(Outcome { output, review })
}
