//! Discover schemas from this invocation's immutable execution catalog.
use super::*;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

struct Entry {
    name: String,
    description: String,
    context: Vec<String>,
}

struct Search {
    session: String,
    stop: CancellationToken,
    entries: Vec<Entry>,
    initial: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    query: String,
    limit: Option<usize>,
}

pub(super) fn bind(
    invocation: &Invocation,
    stop: &CancellationToken,
    tools: &[ToolRegistration],
    instructions: &std::collections::BTreeMap<String, String>,
    loaded: &BTreeSet<String>,
) -> Option<ToolRegistration> {
    let initial = tools
        .iter()
        .filter(|entry| !entry.deferred || loaded.contains(entry.tool.name()))
        .flat_map(|entry| contexts(invocation, instructions, entry))
        .collect();
    let entries: Vec<_> = tools
        .iter()
        .filter(|entry| entry.deferred)
        .map(|entry| Entry {
            name: entry.tool.name().into(),
            description: entry.tool.enhanced_description(),
            context: contexts(invocation, instructions, entry),
        })
        .collect();
    (!entries.is_empty()).then(|| {
        ToolRegistration::managed(Arc::new(Search {
            session: invocation.turn.session.to_string(),
            stop: stop.clone(),
            entries,
            initial,
        }))
    })
}

fn contexts(
    invocation: &Invocation,
    initialized: &std::collections::BTreeMap<String, String>,
    entry: &ToolRegistration,
) -> Vec<String> {
    let mut context = Vec::new();
    if let Some(owner) = &entry.plugin {
        context.extend(
            initialized
                .get(owner)
                .filter(|value| !value.is_empty())
                .cloned(),
        );
        context.extend(
            invocation
                .plugins
                .iter()
                .find(|package| package.summary.name == *owner)
                .and_then(|package| package.extension.as_ref())
                .and_then(|extension| extension.instructions.as_ref())
                .cloned(),
        );
    }
    context.extend(
        invocation
            .instructions
            .iter()
            .filter(|instructions| {
                instructions
                    .any_tools
                    .iter()
                    .any(|name| name == entry.tool.name())
            })
            .map(|instructions| instructions.text.clone()),
    );
    context
}

impl Search {
    fn matching(&self, query: &str, limit: usize) -> Vec<&Entry> {
        let terms: Vec<_> = query
            .split(|character: char| !character.is_alphanumeric())
            .filter(|term| !term.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut matches: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let name = entry.name.to_lowercase();
                let description = entry.description.to_lowercase();
                let score = terms
                    .iter()
                    .map(|term| {
                        usize::from(name.contains(term)) * 4
                            + usize::from(description.contains(term))
                    })
                    .sum::<usize>();
                (score > 0).then_some((score, entry))
            })
            .collect();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| left.name.cmp(&right.name))
        });
        matches
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry)
            .collect()
    }

    fn output(&self, arguments: Arguments, loaded: &mut BTreeSet<String>) -> Value {
        let matched = self.matching(&arguments.query, arguments.limit.unwrap_or(5).clamp(1, 8));
        let previous: BTreeSet<_> = self
            .entries
            .iter()
            .filter(|entry| loaded.contains(&entry.name))
            .flat_map(|entry| entry.context.iter().cloned())
            .collect();
        loaded.extend(matched.iter().map(|entry| entry.name.clone()));
        let context: BTreeSet<_> = matched
            .iter()
            .flat_map(|entry| &entry.context)
            .filter(|context| {
                !previous.contains(context.as_str()) && !self.initial.contains(context.as_str())
            })
            .cloned()
            .collect();
        json!({
            "tools":matched.iter().map(|entry| json!({"name":entry.name,"description":entry.description})).collect::<Vec<_>>(),
            "sailry_context":context.into_iter().collect::<Vec<_>>().join("\n\n"),
        })
    }
}

#[async_trait]
impl Tool for Search {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Find additional tools by name or capability. Matching tools become available on the next model request and stay loaded in this conversation. Search before using a capability absent from the current tool list. Discovery does not execute the tools or change their permissions."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(
            json!({"type":"object","additionalProperties":false,"required":["query"],"properties":{
                "query":{"type":"string","minLength":1},
                "limit":{"type":"integer","minimum":1,"maximum":8}
            }}),
        )
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(AdkError::tool(
                "tool discovery invocation is no longer active",
            ));
        }
        let arguments: Arguments =
            tools::decode(arguments).map_err(|error| AdkError::tool(error.message))?;
        let mut loaded = prompt::session::loaded(
            context
                .session()
                .map(|session| session.state())
                .or_else(|| context.state())
                .and_then(|state| state.get(prompt::session::LOADED_TOOLS))
                .as_ref(),
        );
        let output = self.output(arguments, &mut loaded);
        let mut actions = context.actions();
        actions
            .state_delta
            .insert(prompt::session::LOADED_TOOLS.into(), json!(loaded));
        if let Some(addition) = output["sailry_context"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            let policy = context
                .session()
                .map(|session| session.state())
                .or_else(|| context.state())
                .and_then(|state| state.get(prompt::session::POLICY))
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| AdkError::tool("captured policy is unavailable"))?;
            actions.state_delta.insert(
                prompt::session::POLICY.into(),
                json!(format!("{policy}\n\n{addition}")),
            );
        }
        context.set_actions(actions);
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
