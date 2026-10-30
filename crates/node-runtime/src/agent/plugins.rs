//! Package tools join one execution catalog and lifecycle.
mod failures;
use super::catalog::Registration as ToolRegistration;
use super::*;
pub(super) use failures::public as public_failure;
use std::{collections::BTreeSet, sync::atomic::AtomicBool};
mod assistants;
mod management;
mod operations;
mod scripts;
pub(in crate::agent) mod search;
mod selection;

#[derive(Default)]
pub(super) struct Contribution {
    pub tools: Vec<ToolRegistration>,
    pub instruction: String,
    /// Per-turn plugin script text, delivered as turn context.
    pub context: String,
    pub callbacks: Vec<(
        i16,
        Arc<
            dyn goose_agent::operation::Operation<
                    crate::store::agent::RuntimeSession,
                    super::engine::Effect,
                >,
        >,
    )>,
    pub images: Vec<(Vec<String>, usize)>,
    pub completion: Arc<AtomicBool>,
    loaded: BTreeSet<String>,
    instructions: std::collections::BTreeMap<String, String>,
}

impl Contribution {
    pub(super) fn mounted(&self, name: &str) -> bool {
        self.tools.iter().any(|entry| {
            entry.tool.name.as_ref() == name && (!entry.deferred || self.loaded.contains(name))
        })
    }
}

pub(super) async fn bind(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Invocation,
    loaded: BTreeSet<String>,
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
    let mut bound = Contribution {
        loaded,
        ..Default::default()
    };
    let mut skill_instruction = String::new();
    let mut connection_diagnostics = String::new();
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
            bound.images = connections.screenshots();
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
        bound.instructions = scripts.instructions;
        connection_diagnostics = diagnostics;
    }
    assistants::apply(invocation, &mut bound)?;
    selection::apply(invocation, &mut bound)?;
    selection::defer(invocation, &mut bound.tools)?;
    bound.loaded.retain(|name| {
        bound
            .tools
            .iter()
            .any(|entry| entry.tool.name.as_ref() == name)
    });
    if let Some(search) = search::bind(invocation, stop, &bound.tools, &bound.loaded) {
        bound.tools.push(search);
    }
    if assistant.is_some()
        || bound
            .tools
            .iter()
            .any(|entry| skills::NAMES.contains(&entry.tool.name.as_ref()))
    {
        bound.instruction.push_str(&skill_instruction);
    }
    if invocation.child.is_none()
        || bound
            .tools
            .iter()
            .any(|entry| entry.tool.name.as_ref().starts_with("mcp_"))
    {
        bound.instruction.push_str(&connection_diagnostics);
    }
    for package in &invocation.plugins {
        let mounted = bound.tools.iter().any(|tool| {
            bound.mounted(tool.tool.name.as_ref())
                && tool.plugin.as_deref() == Some(package.summary.name.as_str())
        });
        if let Some(instructions) = bound.instructions.get(&package.summary.name) {
            bound.context.push_str(instructions);
        }
        if mounted
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
                entry.delegation
                    && super::delegation::selected::accepts(
                        &serde_json::Value::Object((*entry.tool.input_schema).clone()),
                        &arguments,
                    )
            })
            .map(|entry| entry.tool.name.as_ref().to_owned())
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

pub(super) fn connection_instructions(
    invocation: &Invocation,
    tools: &[ToolRegistration],
    name: impl Fn(&str) -> String,
) -> String {
    let selected: BTreeSet<_> = tools
        .iter()
        .map(|registration| registration.tool.name.as_ref())
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
        instruction.push_str(&format!("\n- {}: {actions}", name(&alias)));
    }
    instruction.push_str("\nThese tools only read and present current state. Settings and authorization require explicit controller actions. Do not start them, replay a blocked operation, or treat access denial, throttling, timeouts, or unknown failures as authorization evidence.");
    instruction
}
pub(super) mod callbacks;
