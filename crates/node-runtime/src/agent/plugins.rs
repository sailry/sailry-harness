//! Package tools join one execution catalog and lifecycle.
use super::catalog::Registration as ToolRegistration;
use super::*;
use std::collections::BTreeSet;
mod assistants;
mod computer;
mod model_tools;
mod operations;
mod scripts;

#[derive(Default)]
pub(super) struct Contribution {
    pub tools: Vec<ToolRegistration>,
    pub instruction: String,
    pub callbacks: Vec<(i16, adk_core::BeforeModelCallback)>,
}

pub(super) async fn bind(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Invocation,
    stop: &CancellationToken,
    resources: Arc<crate::plugins::resources::Resources>,
    connections: &mut mcp::Connections,
) -> Result<Contribution, Fault> {
    ingress
        .agents
        .bind_media(invocation.turn.id, media::Binding::new(invocation));
    let restricted = invocation.connections.bound.is_some();
    let supports_tools = invocation
        .provider
        .as_ref()
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.id == invocation.turn.config.model)
        })
        .is_some_and(|model| model.tools);
    let mut bound = Contribution::default();
    if !restricted || invocation.turn.config.assistant.is_some() {
        bound.tools.extend(model_tools::bind(invocation)?);
    }
    if supports_tools && (!restricted || invocation.turn.config.assistant.is_some()) {
        bound
            .tools
            .extend(skills::management::bind(ingress, invocation, stop));
    }
    if supports_tools && !restricted {
        let (tools, instruction) =
            skills::bind(resources.clone(), invocation.turn.session, stop.clone())?;
        bound.tools.extend(tools);
        bound.instruction.push_str(&instruction);
    }
    let assistant = invocation
        .turn
        .config
        .assistant
        .as_ref()
        .map(|binding| {
            crate::plugins::conversation::declaration(binding, &invocation.plugins)
                .map(|declaration| (&binding.package, declaration))
        })
        .transpose()?;
    if supports_tools {
        let diagnostics = if !restricted || assistant.is_some() {
            bound
                .tools
                .extend(references::bind(ingress, invocation, stop));
            let (tools, diagnostics) = connections
                .bind(&resources, ingress, invocation, stop, assistant)
                .await;
            bound.tools.extend(tools);
            diagnostics
        } else {
            String::new()
        };
        bound
            .tools
            .extend(operations::bind(ingress, tasks, invocation, stop));
        bound
            .tools
            .extend(computer::bind(ingress, invocation, stop));
        let scripts = scripts::bind(ingress, tasks, invocation, stop).await?;
        bound.tools.extend(scripts.tools);
        bound.callbacks.extend(scripts.callbacks);
        bound.instruction.push_str(&scripts.instruction);
        for package in &invocation.plugins {
            if bound
                .tools
                .iter()
                .any(|tool| tool.plugin.as_deref() == Some(package.summary.name.as_str()))
                && let Some(instructions) = package
                    .extension
                    .as_ref()
                    .and_then(|extension| extension.instructions.as_ref())
            {
                bound.instruction.push_str("\n\n");
                bound.instruction.push_str(instructions);
            }
        }
        bound.instruction.push_str(&diagnostics);
    }
    assistants::apply(invocation, &mut bound)?;
    if invocation.message.selected_role().is_some() {
        let name = bound
            .tools
            .iter()
            .find(|entry| entry.tool.is_agent_delegation())
            .map(|entry| entry.tool.name().to_owned())
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "selected role requires an available delegation tool",
                )
            })?;
        bound
            .callbacks
            .push((-10, super::delegation::selection(invocation, name)?));
    }
    Ok(bound)
}
pub(super) mod callbacks;
