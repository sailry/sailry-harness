//! Product admission and lifecycle around ADK's only model/tool execution loop.
mod approval;
pub(crate) mod attachments;
mod catalog;
mod compaction;
pub(crate) mod completion;
mod controls;
mod delegation;
mod mcp;
pub(crate) mod media;
mod model;
mod plugins;
mod prompt;
mod question;
mod references;
mod skills;
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
use futures::StreamExt;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, TurnId, conversation::Status};
use std::{sync::Arc, time::Duration};

pub(crate) fn start(ingress: Arc<Ingress>, tasks: &Tasks) -> Result<(), Error> {
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
                eprintln!("Agent admission failed: {error}");
                tokio::select! { _ = controls.stopped.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
                continue;
            }
        };
        let _ = run(&ingress, &tasks, invocation, token).await;
    }
}

async fn run(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: Invocation,
    token: CancellationToken,
) -> Result<(Status, Option<String>), Fault> {
    let controls = &ingress.agents;
    let turn = invocation.turn.id;
    let callbacks = plugins::callbacks::capture(&invocation);
    let result = execute(ingress, tasks, invocation, token.clone()).await;
    // Commands and children retain cleanup ownership after ADK drops their tool futures.
    controls.drain(turn).await;
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
    let persisted = ingress.finish_run(turn, status, error.clone()).await;
    if let Err(error) = &persisted {
        eprintln!("Agent completion persistence failed: {error}");
    }
    controls.finish(turn);
    persisted?;
    if !controls.stopped.is_cancelled() {
        plugins::callbacks::complete(ingress, callbacks, status).await;
    }
    controls.wake.notify_waiters();
    if let Some(error) = error {
        return Err(error);
    }
    Ok((status, output))
}

async fn execute(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: Invocation,
    stop: CancellationToken,
) -> Result<Option<String>, Fault> {
    let session = invocation.turn.session;
    let mut connections = ingress.agents.mcp.take(session);
    let result = execute_with(ingress, tasks, invocation, stop.clone(), &mut connections).await;
    // Computer observations belong to the conversation's trusted SDK handle,
    // not this turn. Retain it with the history across completion and stopping.
    // ADK can drop a tool future on interruption before its async cleanup runs.
    // Retain browser cleanup at the same Node-owned boundary as MCP connections.
    let browsers = if stop.is_cancelled() {
        ingress
            .external_browser
            .close(session)
            .await
            .map_err(|error| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    format!("external browser cleanup failed after interruption: {error}"),
                )
            })
    } else {
        Ok(())
    };
    if result.is_ok() && !stop.is_cancelled() && connections.reusable() {
        if let Err(error) = connections.settle().await {
            connections.release().await?;
            return Err(error);
        }
        ingress.agents.mcp.put(session, connections).await?;
    } else {
        connections.close().await?;
    }
    browsers?;
    result
}

async fn execute_with(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: Invocation,
    stop: CancellationToken,
    connections: &mut mcp::Connections,
) -> Result<Option<String>, Fault> {
    if stop.is_cancelled() {
        return Ok(None);
    }
    let model = model::build(ingress, &invocation, &stop).await?;
    if invocation.turn.kind == sailry_protocol::conversation::RunKind::Compaction {
        compaction::manual(ingress, &invocation, model, stop).await?;
        return Ok(None);
    }
    let content = attachments::input(ingress, &invocation).await?;
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
    let mut instruction = prompt::instruction(
        invocation.turn.config.mode,
        invocation.turn.config.permission,
    );
    if let Some(role) = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
    {
        instruction.push_str("\n\n");
        instruction.push_str(&role.instructions);
        for skill in &role.skills {
            let text = resources.read(skill, "SKILL.md").await?;
            instruction.push_str(&format!(
                "\n\nSelected skill {skill}\nDirectory: {}\n{}",
                text.directory, text.content
            ));
        }
    }
    let mut extensions = plugins::bind(
        ingress,
        tasks,
        &invocation,
        &stop,
        resources.clone(),
        connections,
    )
    .await?;
    let restricted = invocation.connections.bound.is_some();
    if restricted {
        instruction.clear();
    }
    instruction.push_str(&extensions.instruction);
    instruction.push_str(
        &references::capabilities::instruction(&invocation, &resources, &extensions.tools).await?,
    );
    let (compact, compaction_config, compact_tool) = compaction::bind(
        ingress,
        model.clone(),
        &invocation,
        stop.clone(),
        instruction.clone(),
    );
    let mut catalog = catalog::Catalog::default();
    let mut agent = adk_agent::LlmAgentBuilder::new("assistant")
        .model(model.clone())
        .instruction_provider(Box::new(move |_| {
            let instruction = instruction.clone();
            Box::pin(async move { Ok(instruction) })
        }));
    extensions.callbacks.push((0, compact));
    extensions.callbacks.sort_by_key(|(order, _)| *order);
    for (_, callback) in extensions.callbacks {
        agent = agent.before_model_callback(callback);
    }
    if let Some(limit) = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
        .and_then(|role| role.max_turns)
    {
        agent = agent.max_iterations(limit);
    }
    if let Some(model) = invocation.provider.as_ref().and_then(|provider| {
        provider
            .models
            .iter()
            .find(|model| model.id == invocation.turn.config.model)
    }) {
        agent = agent.max_output_tokens(model.output as i32);
        if model.tools {
            extensions.tools.extend([
                catalog::Registration::managed(compact_tool),
                catalog::Registration::managed(Arc::new(title::SetTitle {
                    session: invocation.turn.session.to_string(),
                    stop: stop.clone(),
                })),
                catalog::Registration::managed(Arc::new(question::Question {
                    ingress: ingress.clone(),
                    turn: invocation.turn.id,
                    session: invocation.turn.session.to_string(),
                    stop: stop.clone(),
                    planning: planning && invocation.child.is_none(),
                })),
            ]);
            agent = agent
                .parallelize_agent_delegations(true)
                .tool_execution_strategy(adk_core::ToolExecutionStrategy::Auto);
        }
    }
    for registration in extensions.tools {
        if planning && !registration.managed && !registration.tool.is_read_only() {
            continue;
        }
        let confirmation = !registration.managed && !registration.tool.is_read_only();
        let tool = catalog.register(registration)?;
        if confirmation {
            agent = agent.require_tool_confirmation(tool.name());
        }
        agent = agent.tool(tool);
    }
    let agent = agent.build().map_err(model::error)?;
    let runner = Runner::builder()
        .app_name(APP)
        .agent(Arc::new(agent))
        .session_service(Arc::new(
            crate::store::agent::Sessions::new(ingress.clone(), invocation.turn.id)
                .with_references(invocation.message.references.clone())
                .with_groupings(catalog.groupings())
                .with_displays(catalog.displays())
                .with_presentations(catalog.presentations()),
        ))
        .compaction_config(compaction_config)
        .run_config(RunConfig {
            tool_confirmation_handler: Some(Arc::new(approval::Handler {
                ingress: ingress.clone(),
                turn: invocation.turn.id,
                stop: stop.clone(),
            })),
            ..Default::default()
        });
    let runner = runner.build().map_err(model::error)?;
    let session = invocation.turn.session.to_string();
    let target = invocation.turn.session;
    let turn = invocation.turn.id;
    let user = adk_core::UserId::try_from(USER).map_err(|error| model::error(error.into()))?;
    let session_id = adk_core::SessionId::try_from(session.as_str())
        .map_err(|error| model::error(error.into()))?;
    let mut events = runner
        .run(user, session_id, content)
        .await
        .map_err(model::error)?;
    let mut cancelled = false;
    let mut output = None;
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
                // Runner has already committed non-partial events through Sessions.
                if event.llm_response.error_code.is_some()
                    || event.llm_response.error_message.is_some()
                {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "model response reported an error",
                    ));
                }
                if event.llm_response.interrupted && !stop.is_cancelled() {
                    return Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "model response was interrupted",
                    ));
                }
                if event.llm_response.partial {
                    ingress.partial(target, turn, event).await?;
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
            None => return Ok(output),
        }
    }
}
