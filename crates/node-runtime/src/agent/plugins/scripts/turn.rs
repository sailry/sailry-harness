//! Bounded package policy shares one ephemeral owner with its captured tool handlers.
//! It can inspect its own visible results, never modify model messages or history.
use super::*;
use crate::{
    agent::{
        engine::{
            conversation::{EffectiveRole, effective_role},
            operation::{Callback, Emitter, Outcome, not_applicable, yielded},
        },
        model::{Content, Message},
    },
    store::agent::RuntimeSession,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};
mod catalog;

/// Discrete context parts one initializer may return, and their combined UTF-8 size.
const MAX_PARTS: usize = 8;
const MAX_PARTS_BYTES: usize = 32 * 1024;

pub(super) struct Turn {
    pub state: Mutex<Value>,
    ingress: Arc<Ingress>,
    caller: NodeId,
    package: plugin::Info,
    context: plugin::Context,
    mode: sailry_protocol::WorkMode,
    stop: CancellationToken,
    names: BTreeMap<String, String>,
}

pub(super) struct Initialized {
    pub owner: Arc<Turn>,
    pub tools: Vec<String>,
    pub instruction: String,
    /// Distinct parts, each persisted at most once while visible in the conversation.
    pub parts: Vec<String>,
    pub parameters: BTreeMap<String, Value>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    instruction: String,
    #[serde(default)]
    parts: Vec<String>,
    tools: Vec<String>,
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
}

impl Selection {
    fn validate(&self, available: &[String]) -> Result<(), Fault> {
        if self.tools.iter().any(|name| !available.contains(name))
            || self.parameters.iter().any(|(name, schema)| {
                !self.tools.contains(name)
                    || !schema.is_object()
                    || schema["type"] != "object"
                    || !serde_json::to_vec(schema)
                        .is_ok_and(|bytes| bytes.len() <= plugin::host::MAX_DATA_BYTES)
            })
        {
            return Err(sailry_protocol::Fault::invalid(
                "plugin turn selected an unavailable tool or invalid schema",
            ));
        }
        if self.parts.len() > MAX_PARTS
            || self.parts.iter().any(|part| part.trim().is_empty())
            || self.parts.iter().map(String::len).sum::<usize>() > MAX_PARTS_BYTES
        {
            return Err(sailry_protocol::Fault::invalid(
                "plugin turn returned invalid context parts",
            ));
        }
        Ok(())
    }

    /// The returned parts in order, without repeats.
    fn distinct(&self) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        for part in &self.parts {
            if !parts.contains(part) {
                parts.push(part.clone());
            }
        }
        parts
    }
}

impl Turn {
    pub async fn initialize(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        package: &plugin::Info,
        tools: Vec<String>,
        stop: &CancellationToken,
    ) -> Result<Initialized, Fault> {
        let policy = package
            .extension
            .as_ref()
            .unwrap()
            .host
            .as_ref()
            .unwrap()
            .turn
            .as_ref()
            .unwrap();
        let owner = Arc::new(Self {
            state: Mutex::new(Value::Null),
            ingress: ingress.clone(),
            caller: invocation.caller,
            context: plugin::Context {
                invocation: None,
                turn: Some(invocation.turn.id),
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(invocation.worktree),
                session: Some(invocation.turn.session),
            },
            package: package.clone(),
            mode: invocation.turn.config.mode,
            stop: stop.clone(),
            names: tools
                .iter()
                .map(|name| (operations::alias(&package.summary.name, name), name.clone()))
                .collect(),
        });
        let resources = crate::plugins::conversation::owns_resources(
            invocation.turn.config.assistant.as_ref(),
            package,
        );
        let mut input = catalog::input(invocation, package, &tools)?;
        input["tools"] = json!(tools);
        input["project"] = json!(invocation.project);
        input["resources"] = json!(resources);
        let call = super::identity::call(
            invocation.turn.id,
            &owner.context.package,
            "turn.initialize",
            &policy.initialize,
        )?;
        let result = owner
            .evaluate(policy.initialize.clone(), input, Some(call))
            .await?;
        let result: Selection = serde_json::from_value(result)
            .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin turn selection"))?;
        result.validate(&tools)?;
        Ok(Initialized {
            owner,
            parts: result.distinct(),
            tools: result.tools,
            instruction: result.instruction,
            parameters: result.parameters,
        })
    }

    pub fn observer(self: &Arc<Self>, completion: Arc<AtomicBool>) -> Option<Arc<Observer>> {
        let handler = self
            .package
            .extension
            .as_ref()?
            .host
            .as_ref()?
            .turn
            .as_ref()?
            .before_model
            .clone()?;
        Some(Arc::new(Observer {
            owner: self.clone(),
            handler,
            completion,
        }))
    }

    /// A hook's stable identity lets it write private package storage once per evaluation.
    async fn evaluate(
        self: &Arc<Self>,
        handler: String,
        input: Value,
        call: Option<sailry_protocol::RequestId>,
    ) -> Result<Value, Fault> {
        if serde_json::to_vec(&input)
            .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin turn input"))?
            .len()
            > plugin::host::MAX_DATA_BYTES
        {
            return Err(sailry_protocol::Fault::invalid(
                "plugin turn input exceeds limit",
            ));
        }
        if self.stop.is_cancelled() {
            return Err(Fault::new(ErrorCode::Cancelled, "plugin turn stopped"));
        }
        let owner = self.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let mut current = owner
                .state
                .lock()
                .map_err(|_| sailry_protocol::Fault::invalid("plugin turn state is unavailable"))?;
            let bundle = owner
                .ingress
                .plugins
                .read_host(&owner.package, owner.stop.clone())?;
            let state = Arc::new(Mutex::new(crate::plugins::script::execution::State {
                call_id: call,
                mode: owner.mode,
                read_only: false,
                turn: Some(current.clone()),
                staged_turn: None,
            }));
            let ingress = owner.ingress.clone();
            let caller = owner.caller;
            let deadline = ingress.plugins.script_deadline();
            let stopped = owner.stop.clone();
            let report_runtime = runtime.clone();
            let result = crate::plugins::script::run_tool(
                bundle,
                &handler,
                input,
                crate::plugins::script::Environment {
                    context: owner.context.clone(),
                    target: ingress.node,
                    stop: owner.stop.clone(),
                    deadline,
                },
                move |request| {
                    runtime.block_on(ingress.script_dispatch(caller, request, deadline, &stopped))
                },
                state.clone(),
            )
            .map_err(|fault| {
                report_runtime.block_on(owner.ingress.script_failure(caller, fault, &owner.package))
            })?;
            if let Some(staged) = state
                .lock()
                .map_err(|_| sailry_protocol::Fault::invalid("plugin turn state is unavailable"))?
                .staged_turn
                .take()
            {
                *current = staged;
            }
            Ok(result)
        })
        .await
        .map_err(|_| sailry_protocol::Fault::invalid("plugin turn worker failed"))?
    }
}

pub(super) struct Observer {
    owner: Arc<Turn>,
    handler: String,
    completion: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl Callback for Observer {
    async fn run(
        &self,
        _: &RuntimeSession,
        conversation: &[Message],
        _: &Emitter,
    ) -> anyhow::Result<Outcome> {
        if conversation
            .last()
            .is_none_or(|last| effective_role(last) == EffectiveRole::Assistant)
        {
            return not_applicable();
        }
        let calls: BTreeMap<_, _> = conversation
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| {
                let Content::ToolCall(request) = block else {
                    return None;
                };
                request
                    .error
                    .is_none()
                    .then_some((request.id.as_str(), request.name.as_str()))
            })
            .collect();
        let responses: Vec<_> = conversation
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| match block {
                Content::ToolResult(response) => Some(response),
                _ => None,
            })
            .collect();
        // The newest result identifies this observation across replays.
        let call = responses
            .last()
            .zip(self.owner.context.turn)
            .map(|(response, turn)| {
                super::identity::call(
                    turn,
                    &self.owner.context.package,
                    "turn.before_model",
                    response.id.as_str(),
                )
            })
            .transpose()?;
        let results: Vec<_> = responses
            .into_iter()
            .filter_map(|response| {
                let name = self.owner.names.get(*calls.get(response.id.as_str())?)?;
                let value = match &response.result {
                    Ok(result) => result
                        .structured_content
                        .clone()
                        .unwrap_or_else(|| serde_json::to_value(result).unwrap()),
                    Err(error) => serde_json::to_value(error).unwrap(),
                };
                Some(json!({"name":name,"response":value}))
            })
            .collect();
        let result = self
            .owner
            .evaluate(self.handler.clone(), json!({"results":results}), call)
            .await?;
        if result.get("stop").and_then(Value::as_bool) == Some(true) {
            self.completion.store(true, Ordering::Release);
            return yielded();
        }
        not_applicable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_only_replace_selected_tools() {
        let mut selection: Selection = serde_json::from_value(json!({
            "tools":["query"], "instruction":"", "parameters":{
                "query":{"type":"object","properties":{"connection":{"enum":["captured"]}}}
            }
        }))
        .unwrap();
        let available = vec!["query".into(), "execute".into()];
        selection.validate(&available).unwrap();
        selection
            .parameters
            .insert("execute".into(), json!({"type":"object"}));
        assert!(selection.validate(&available).is_err());
        selection.parameters.remove("execute");
        selection.tools.push("unavailable".into());
        assert!(selection.validate(&available).is_err());
    }

    #[test]
    fn parts_are_bounded_text() {
        let available = vec!["query".to_owned()];
        let selection = |parts: Value| -> Selection {
            serde_json::from_value(json!({"tools":["query"],"instruction":"","parts":parts}))
                .unwrap()
        };
        let absent: Selection =
            serde_json::from_value(json!({"tools":["query"],"instruction":""})).unwrap();
        absent.validate(&available).unwrap();
        assert!(absent.distinct().is_empty());
        let repeated = selection(json!(["First", "Second", "First"]));
        repeated.validate(&available).unwrap();
        assert_eq!(repeated.distinct(), ["First", "Second"]);
        selection(json!(vec!["x"; MAX_PARTS]))
            .validate(&available)
            .unwrap();
        selection(json!(["a".repeat(MAX_PARTS_BYTES)]))
            .validate(&available)
            .unwrap();
        for invalid in [
            json!(vec!["x"; MAX_PARTS + 1]),
            json!([""]),
            json!([" \n"]),
            json!(["a".repeat(MAX_PARTS_BYTES + 1)]),
            json!([
                "a".repeat(MAX_PARTS_BYTES / 2 + 1),
                "b".repeat(MAX_PARTS_BYTES / 2)
            ]),
        ] {
            assert!(selection(invalid).validate(&available).is_err());
        }
        assert!(
            serde_json::from_value::<Selection>(json!({"tools":[],"instruction":"","parts":[1]}))
                .is_err()
        );
    }

    #[test]
    fn full_catalog_labels_fit_envelope() {
        let mut selection: Selection = serde_json::from_value(json!({
            "tools":["query"],"instruction":"","parameters":{
                "query":{"type":"object","description":"label ".repeat(4096)}
            }
        }))
        .unwrap();
        let available = vec!["query".into()];
        selection.validate(&available).unwrap();
        selection
            .parameters
            .insert("query".into(), json!({"type":"string"}));
        assert!(selection.validate(&available).is_err());
        selection.parameters.insert(
            "query".into(),
            json!({"type":"object","description":"x".repeat(plugin::host::MAX_DATA_BYTES)}),
        );
        assert!(selection.validate(&available).is_err());
    }
}
