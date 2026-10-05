//! Bounded package policy shares one ephemeral owner with its captured tool handlers.
//! It can inspect its own visible results, never modify model messages or history.
use super::*;
use adk_core::{BeforeModelCallback, BeforeModelResult, Part};
use std::collections::BTreeMap;
mod catalog;

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
    pub parameters: BTreeMap<String, Value>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    instruction: String,
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
            return Err(invalid(
                "plugin turn selected an unavailable tool or invalid schema",
            ));
        }
        Ok(())
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
        let mut input = catalog::input(invocation, package, &tools);
        input["tools"] = json!(tools);
        input["project"] = json!(invocation.project);
        input["resources"] = json!(resources);
        let result = owner.evaluate(policy.initialize.clone(), input).await?;
        let result: Selection =
            serde_json::from_value(result).map_err(|_| invalid("invalid plugin turn selection"))?;
        result.validate(&tools)?;
        Ok(Initialized {
            owner,
            tools: result.tools,
            instruction: if resources {
                result.instruction
            } else {
                String::new()
            },
            parameters: result.parameters,
        })
    }

    pub fn observer(self: &Arc<Self>) -> Option<BeforeModelCallback> {
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
        let owner = self.clone();
        Some(Box::new(move |_, request| {
            let results: Vec<_> = request
                .contents
                .iter()
                .flat_map(|content| &content.parts)
                .filter_map(|part| {
                    let Part::FunctionResponse {
                        function_response, ..
                    } = part
                    else {
                        return None;
                    };
                    owner
                        .names
                        .get(&function_response.name)
                        .map(|name| json!({"name":name,"response":function_response.response}))
                })
                .collect();
            let owner = owner.clone();
            let handler = handler.clone();
            Box::pin(async move {
                let result = owner
                    .evaluate(handler, json!({"results":results}))
                    .await
                    .map_err(|fault| AdkError::tool(fault.message))?;
                if result.get("stop").and_then(Value::as_bool) == Some(true) {
                    return Ok(BeforeModelResult::Skip(adk_core::LlmResponse {
                        turn_complete: true,
                        finish_reason: Some(adk_core::FinishReason::Stop),
                        ..Default::default()
                    }));
                }
                Ok(BeforeModelResult::Continue(request))
            })
        }))
    }

    async fn evaluate(self: &Arc<Self>, handler: String, input: Value) -> Result<Value, Fault> {
        if serde_json::to_vec(&input)
            .map_err(|_| invalid("invalid plugin turn input"))?
            .len()
            > plugin::host::MAX_DATA_BYTES
        {
            return Err(invalid("plugin turn input exceeds limit"));
        }
        let permit = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin turn stopped")),
            permit = self.ingress.plugins.scripts.clone().acquire_owned() => permit.map_err(|_| invalid("plugin VM pool is closed"))?,
        };
        let owner = self.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut current = owner
                .state
                .lock()
                .map_err(|_| invalid("plugin turn state is unavailable"))?;
            let bundle = owner
                .ingress
                .plugins
                .read_host(&owner.package, owner.stop.clone())?;
            let state = Arc::new(Mutex::new(crate::plugins::script::execution::State {
                call_id: None,
                mode: owner.mode,
                read_only: false,
                turn: Some(current.clone()),
                staged_turn: None,
            }));
            let ingress = owner.ingress.clone();
            let caller = owner.caller;
            let result = crate::plugins::script::run_tool(
                bundle,
                &handler,
                input,
                crate::plugins::script::Environment {
                    context: owner.context.clone(),
                    target: ingress.node,
                    stop: owner.stop.clone(),
                },
                move |request| {
                    runtime.block_on(async {
                        ingress
                            .dispatch(caller, request)
                            .await?
                            .completion
                            .await
                            .map_err(|_| {
                                Fault::new(
                                    ErrorCode::OutcomeUnknown,
                                    "plugin SDK completion is unknown",
                                )
                            })?
                    })
                },
                state.clone(),
            )?;
            if let Some(staged) = state
                .lock()
                .map_err(|_| invalid("plugin turn state is unavailable"))?
                .staged_turn
                .take()
            {
                *current = staged;
            }
            Ok(result)
        })
        .await
        .map_err(|_| invalid("plugin turn worker failed"))?
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
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
