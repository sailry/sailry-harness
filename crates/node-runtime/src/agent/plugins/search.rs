//! Discover schemas from this invocation's immutable execution catalog.
use super::*;
use crate::agent::catalog::CallContext;
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
    loaded: &BTreeSet<String>,
) -> Option<ToolRegistration> {
    let initial = tools
        .iter()
        .filter(|entry| !entry.deferred || loaded.contains(entry.tool.name.as_ref()))
        .flat_map(|entry| contexts(invocation, entry))
        .collect();
    let entries: Vec<_> = tools
        .iter()
        .filter(|entry| entry.deferred)
        .map(|entry| Entry {
            name: entry.tool.name.as_ref().into(),
            description: entry
                .tool
                .description
                .as_deref()
                .unwrap_or_default()
                .to_owned(),
            context: contexts(invocation, entry),
        })
        .collect();
    (!entries.is_empty()).then(|| {
        let tool = Arc::new(Search {
            session: invocation.turn.session.to_string(),
            stop: stop.clone(),
            entries,
            initial,
        });
        ToolRegistration::managed(tool.descriptor(), tool)
    })
}

fn contexts(invocation: &Invocation, entry: &ToolRegistration) -> Vec<String> {
    let mut context = Vec::new();
    if let Some(owner) = &entry.plugin {
        // Per-turn script instructions already live in the request-only tail.
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
                    .any(|name| name == entry.tool.name.as_ref())
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

    fn output(&self, arguments: Arguments, loaded: &mut Vec<String>) -> Value {
        let matched = self.matching(&arguments.query, arguments.limit.unwrap_or(5).clamp(1, 8));
        let previous: BTreeSet<_> = self
            .entries
            .iter()
            .filter(|entry| loaded.contains(&entry.name))
            .flat_map(|entry| entry.context.iter().cloned())
            .collect();
        for entry in &matched {
            if !loaded.contains(&entry.name) {
                loaded.push(entry.name.clone());
            }
        }
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

impl Search {
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
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(Fault::invalid(
                "tool discovery invocation is no longer active",
            ));
        }
        let arguments: Arguments =
            tools::decode(arguments).map_err(|error| Fault::invalid(error.message))?;
        let mut loaded = context.context.loaded_tools.clone().unwrap_or_default();
        let output = self.output(arguments, &mut loaded);
        let mut delta = crate::agent::canonical::Context {
            loaded_tools: Some(loaded),
            ..Default::default()
        };
        if let Some(addition) = output["sailry_context"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            let policy = context
                .context
                .policy
                .as_deref()
                .ok_or_else(|| Fault::invalid("captured policy is unavailable"))?;
            delta.policy = Some(format!("{policy}\n\n{addition}"));
        }
        crate::agent::catalog::with_context(crate::agent::catalog::json(output)?, delta)
    }
}

/// Only the discovery owner projects its name fields; descriptions and package
/// context remain untouched, including text which resembles a tool alias.
pub(in crate::agent) fn project(
    result: &mut rmcp::model::CallToolResult,
    name: impl Fn(&str) -> String,
) {
    let Some(original) = &result.structured_content else {
        return;
    };
    let mut projected = original.clone();
    let Some(tools) = projected.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        let Some(value) = tool.get_mut("name").filter(|value| value.is_string()) else {
            return;
        };
        *value = Value::String(name(value.as_str().unwrap()));
    }
    if projected == *original {
        return;
    }
    let encoded = original.to_string();
    let updated = projected.to_string();
    let mut mirrored = false;
    for content in &mut result.content {
        if let rmcp::model::ContentBlock::Text(text) = content
            && (text.text == encoded
                || serde_json::from_str::<Value>(&text.text).is_ok_and(|value| &value == original))
        {
            text.text.clone_from(&updated);
            mirrored = true;
        }
    }
    if mirrored {
        result.structured_content = Some(projected);
    }
}

#[cfg(test)]
mod tests;

crate::agent::catalog::provider!(Search, crate::agent::catalog::result);
