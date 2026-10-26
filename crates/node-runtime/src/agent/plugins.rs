//! Package tools join one execution catalog and lifecycle.
mod failures;
use super::catalog::Registration as ToolRegistration;
use super::*;
pub(super) use failures::{bind as failure_callback, public as public_failure};
use std::{collections::BTreeSet, sync::atomic::AtomicBool};
mod assistants;
mod management;
mod model_tools;
mod operations;
mod scripts;
mod selection;

#[derive(Default)]
pub(super) struct Contribution {
    pub tools: Vec<ToolRegistration>,
    pub instruction: String,
    pub callbacks: Vec<(i16, adk_core::BeforeModelCallback)>,
    pub completion: Arc<AtomicBool>,
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
        .is_none_or(|model| model.tools)
        && invocation
            .provider
            .as_ref()
            .and_then(|provider| provider.binding.as_ref())
            .and_then(|binding| binding.adapter.as_ref())
            .is_none_or(|adapter| adapter.capabilities.tools);
    let mut bound = Contribution::default();
    let mut skill_instruction = String::new();
    let mut connection_diagnostics = String::new();
    if !restricted || invocation.turn.config.assistant.is_some() {
        bound.tools.extend(model_tools::bind(ingress, invocation)?);
    }
    if supports_tools && (!restricted || invocation.turn.config.assistant.is_some()) {
        bound
            .tools
            .extend(skills::management::bind(ingress, invocation, stop));
        bound
            .tools
            .extend(mcp::management::bind(ingress, invocation, stop));
        bound
            .tools
            .extend(management::bind(ingress, invocation, stop));
    }
    if supports_tools && !restricted {
        let (tools, instruction) = skills::bind(
            resources.clone(),
            invocation.turn.session,
            stop.clone(),
            ingress.clone(),
            invocation.turn.id,
        )?;
        bound.tools.extend(tools);
        skill_instruction = instruction;
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
        bound
            .tools
            .extend(references::bind(ingress, invocation, stop));
        let diagnostics = if !restricted || assistant.is_some() {
            let (tools, diagnostics) = connections
                .bind(&resources, ingress, invocation, stop, assistant)
                .await;
            bound.tools.extend(tools);
            if let Some(callback) = connections.screenshots() {
                // After compaction (0) and package observers (10).
                bound.callbacks.push((20, callback));
            }
            diagnostics
        } else {
            String::new()
        };
        bound
            .tools
            .extend(operations::bind(ingress, tasks, invocation, stop));
        let scripts = scripts::bind(ingress, tasks, invocation, stop).await?;
        bound.completion = scripts.completion;
        bound.tools.extend(scripts.tools);
        bound.callbacks.extend(scripts.callbacks);
        bound.instruction.push_str(&scripts.instruction);
        connection_diagnostics = diagnostics;
    }
    assistants::apply(invocation, &mut bound)?;
    selection::apply(invocation, &mut bound)?;
    if assistant.is_some()
        || bound
            .tools
            .iter()
            .any(|entry| skills::NAMES.contains(&entry.tool.name()))
    {
        bound.instruction.push_str(&skill_instruction);
    }
    if invocation.child.is_none()
        || bound
            .tools
            .iter()
            .any(|entry| entry.tool.name().starts_with("mcp_"))
    {
        bound.instruction.push_str(&connection_diagnostics);
    }
    if invocation.child.is_none() {
        bound
            .instruction
            .push_str(&connection_instructions(invocation, &bound.tools));
    }
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
    if invocation.message.selected_role().is_some() {
        let arguments = super::delegation::selected::arguments(invocation)?;
        let name = bound
            .tools
            .iter()
            .find(|entry| {
                entry.tool.is_agent_delegation()
                    && entry.tool.parameters_schema().is_some_and(|schema| {
                        super::delegation::selected::accepts(&schema, &arguments)
                    })
            })
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

fn connection_instructions(invocation: &Invocation, tools: &[ToolRegistration]) -> String {
    let selected: BTreeSet<_> = tools
        .iter()
        .map(|registration| registration.tool.name())
        .collect();
    let mut cards = BTreeSet::new();
    for package in &invocation.plugins {
        let Some(extension) = &package.extension else {
            continue;
        };
        for card in &extension.cards {
            let Some(tool) = &card.tool else { continue };
            let alias = operations::alias(&package.summary.name, tool);
            if selected.contains(alias.as_str()) {
                cards.insert((alias, card.capabilities.settings, card.mcp_authorization()));
            }
        }
    }
    if cards.is_empty() {
        return String::new();
    }
    let mut instruction = String::from("\n\nDeclared connection cards in this tool catalog:");
    for (alias, settings, authorization) in cards {
        let actions = match (settings, authorization) {
            (true, true) => "settings and MCP authorization",
            (true, false) => "settings",
            (false, true) => "MCP authorization",
            (false, false) => "no setup action",
        };
        instruction.push_str(&format!("\n- {alias}: {actions}"));
    }
    instruction.push_str("\nThese tools only read and present current state. Settings and authorization require explicit controller actions. Do not start them, replay a blocked operation, or treat access denial, throttling, timeouts, or unknown failures as authorization evidence.");
    instruction
}
pub(super) mod callbacks;
