//! Canonical Cua tools join the admitted package and Node command boundary.
use super::*;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, NodeId, Output, Request, plugin};
use serde_json::{Value, json};

pub(super) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<ToolRegistration> {
    if invocation.connections.bound.is_some() && invocation.turn.config.assistant.is_none() {
        return Vec::new();
    }
    let mut tools = Vec::new();
    for package in &invocation.plugins {
        if !crate::plugins::tools::owns_computer(package) {
            continue;
        }
        for definition in crate::computer::catalog()
            .as_array()
            .expect("canonical tool catalog")
        {
            let name = definition["name"].as_str().expect("canonical tool name");
            if crate::plugins::tools::computer(package, name).is_none()
                || !crate::plugins::conversation::selects(
                    invocation.turn.config.assistant.as_ref(),
                    &invocation.plugins,
                    package,
                    name,
                )
            {
                continue;
            }
            let mut registration = ToolRegistration::new(
                Arc::new(Bound {
                    ingress: ingress.clone(),
                    transport: Arc::new(Local::new(
                        ingress.node,
                        invocation.caller,
                        ingress.clone(),
                    )),
                    caller: invocation.caller,
                    turn: invocation.turn.id,
                    context: plugin::Context {
                        invocation: None,
                        turn: Some(invocation.turn.id),
                        surface: plugin::desktop::Surface::Workspace,
                        package: package.summary.reference(),
                        worktree: Some(invocation.worktree),
                        session: Some(invocation.turn.session),
                    },
                    definition,
                    stop: stop.clone(),
                }),
                Default::default(),
            );
            registration.plugin = Some(package.summary.name.clone());
            tools.push(registration);
        }
    }
    tools
}

struct Bound {
    ingress: Arc<Ingress>,
    transport: Arc<dyn Transport>,
    caller: NodeId,
    turn: sailry_protocol::TurnId,
    context: plugin::Context,
    definition: &'static Value,
    stop: CancellationToken,
}

#[async_trait]
impl Tool for Bound {
    fn name(&self) -> &str {
        self.definition["name"]
            .as_str()
            .expect("canonical tool name")
    }
    fn description(&self) -> &str {
        self.definition["description"]
            .as_str()
            .expect("canonical tool description")
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(parameters(self.definition))
    }
    fn is_read_only(&self) -> bool {
        self.definition["annotations"]["readOnlyHint"]
            .as_bool()
            .expect("canonical tool annotation")
    }
    fn is_concurrency_safe(&self) -> bool {
        self.is_read_only()
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if Some(context.session_id()) != self.context.session.map(|id| id.to_string()).as_deref() {
            return Err(AdkError::tool("plugin tool belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("plugin tool was cancelled"));
        }
        let work = async {
            let mut request = Request::new(
                self.transport.target(),
                Command::UseComputer {
                    session: self.context.session.expect("captured session"),
                    worktree: self.context.worktree.expect("captured worktree"),
                    name: self.name().into(),
                    arguments: arguments.clone(),
                },
            )
            .with_plugin(self.context.clone());
            self.ingress
                .check_plugin(self.caller, request.clone())
                .await?;
            if !self.is_read_only() {
                request.id = self
                    .ingress
                    .authorize(
                        self.turn,
                        adk_core::ToolConfirmationRequest {
                            tool_name: self.name().into(),
                            function_call_id: Some(context.function_call_id().into()),
                            args: arguments,
                        },
                    )
                    .await?;
            }
            let admission = self.transport.dispatch(request).await?;
            let output = tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "tool operation was cancelled; its outcome may be uncertain",
                )),
                result = admission.completion => result.map_err(|_| Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "tool operation outcome is unavailable",
                ))??,
            };
            match output {
                Output::Computer(value) => Ok(value),
                _ => Err(Fault::new(
                    ErrorCode::Internal,
                    "unexpected tool operation output",
                )),
            }
        };
        match work.await {
            Ok(output) => media(output),
            Err(error) => Ok(json!({"error":error,"isError":true})),
        }
    }
}

fn parameters(definition: &Value) -> Value {
    let mut schema = definition["inputSchema"].clone();
    // The trusted SDK handle already binds this lifecycle parameter and inserts
    // it before native validation. Do not ask the model to select another label.
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.remove("session");
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|name| name != "session");
    }
    schema
}

/// ADK requires binary tool media outside the JSON response. All other MCP
/// fields remain unchanged; the Node wire result retains its original bytes.
fn media(mut output: Value) -> adk_core::Result<Value> {
    let mut inline = Vec::new();
    if let Some(content) = output.get_mut("content").and_then(Value::as_array_mut) {
        for item in content {
            if !matches!(item["type"].as_str(), Some("image" | "audio")) {
                continue;
            }
            let mime_type = item["mimeType"]
                .as_str()
                .ok_or_else(|| AdkError::tool("tool media omitted its MIME type"))?;
            let data = item["data"]
                .as_str()
                .ok_or_else(|| AdkError::tool("tool media omitted its data"))?;
            let bytes = STANDARD
                .decode(data)
                .map_err(|error| AdkError::tool(format!("invalid tool media: {error}")))?;
            let mut part = json!({"mime_type":mime_type,"data":bytes});
            if let Some(annotations) = item.get("annotations") {
                part["annotations"] = annotations.clone();
            }
            inline.push(part);
            item.as_object_mut()
                .expect("MCP content object")
                .remove("data");
        }
    }
    if inline.is_empty() {
        Ok(output)
    } else {
        Ok(json!({"response":output,"inline_data":inline}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_only_the_session_parameter() {
        for definition in crate::computer::catalog().as_array().unwrap() {
            let original = &definition["inputSchema"];
            let mut projected = parameters(definition);
            assert!(projected["properties"].get("session").is_none());
            assert!(
                projected["required"]
                    .as_array()
                    .is_none_or(|required| !required.iter().any(|name| name == "session"))
            );
            if let Some(session) = original["properties"].get("session") {
                projected["properties"]["session"] = session.clone();
            }
            if let Some(required) = original.get("required") {
                projected["required"] = required.clone();
            }
            assert_eq!(&projected, original, "{}", definition["name"]);
        }
    }

    #[test]
    fn preserves_plain_results() {
        let raw = json!({
            "isError":true,
            "structuredContent":{"code":"refused","degraded":true},
            "content":[{"type":"text","text":"Original result","annotations":{"priority":0.5}}],
            "_meta":{"source":"driver"},
        });
        assert_eq!(media(raw.clone()).unwrap(), raw);
    }

    #[test]
    fn relocates_only_binary_data() {
        let raw = json!({
            "isError":false,
            "structuredContent":{"snapshot_id":"native-snapshot","effect":"partial"},
            "content":[
                {"type":"text","text":"Original result"},
                {"type":"image","mimeType":"image/png","data":"AQID","annotations":{"priority":0.5},"_meta":{"source":"driver"}},
                {"type":"audio","mimeType":"audio/wav","data":"BAU="},
            ],
            "_meta":{"result":"native"},
        });
        let adapted = media(raw.clone()).unwrap();
        let response = adk_core::FunctionResponseData::from_tool_result("capture", adapted);
        let mut expected = raw;
        for index in [1, 2] {
            expected["content"][index]
                .as_object_mut()
                .unwrap()
                .remove("data");
        }
        assert_eq!(response.response, expected);
        assert_eq!(response.inline_data[0].data, [1, 2, 3]);
        assert_eq!(response.inline_data[0].mime_type, "image/png");
        assert_eq!(
            response.inline_data[0].annotations,
            Some(json!({"priority":0.5}))
        );
        assert_eq!(response.inline_data[1].data, [4, 5]);
    }

    #[test]
    fn reports_invalid_media() {
        assert!(
            media(json!({"content":[{"type":"image","mimeType":"image/png","data":"invalid!"}]}))
                .is_err()
        );
    }
}
