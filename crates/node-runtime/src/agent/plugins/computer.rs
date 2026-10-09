//! Canonical Cua tools join the admitted package and Node command boundary.
use super::*;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, NodeId, Output, Request, plugin};
use serde_json::{Value, json};

/// Screenshots retained in each model request. Older ones are omitted in
/// batches so cached request prefixes stay stable between removals.
const SCREENSHOTS: usize = 3;
const BATCH: usize = 5;

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
        if !crate::plugins::tools::owns_computer(package) || !ingress.computer.available() {
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
        crate::computer::read_only(self.name()).expect("canonical tool annotation")
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
                        Some(request.command.clone()),
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
    // The trusted SDK handle binds the lifecycle session, and host path
    // parameters would bypass worktree confinement. Admission rejects both.
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.retain(|name, _| !crate::computer::HIDDEN.contains(&name.as_str()));
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|name| {
            name.as_str()
                .is_none_or(|name| !crate::computer::HIDDEN.contains(&name))
        });
    }
    schema
}

/// Omit older computer screenshots from the model request only. The session
/// history keeps every original result; user attachments are never touched.
/// Runs after compaction, which matches requests against persisted history.
pub(super) fn screenshots(tools: Vec<String>) -> adk_core::BeforeModelCallback {
    Box::new(move |_, mut request| {
        omit(&mut request, &tools);
        Box::pin(async move { Ok(adk_core::BeforeModelResult::Continue(request)) })
    })
}

fn omit(request: &mut adk_core::LlmRequest, tools: &[String]) {
    let image = |data: &adk_core::InlineDataPart| data.mime_type.starts_with("image/");
    let mut responses: Vec<_> = request
        .contents
        .iter_mut()
        .flat_map(|content| content.parts.iter_mut())
        .filter_map(|part| match part {
            adk_core::Part::FunctionResponse {
                function_response, ..
            } if tools.contains(&function_response.name)
                && function_response.inline_data.iter().any(image) =>
            {
                Some(function_response)
            }
            _ => None,
        })
        .collect();
    let total: usize = responses
        .iter()
        .map(|response| {
            response
                .inline_data
                .iter()
                .filter(|data| image(data))
                .count()
        })
        .sum();
    let mut remaining = total.saturating_sub(SCREENSHOTS) / BATCH * BATCH;
    if remaining == 0 {
        return;
    }
    for response in &mut responses {
        if remaining == 0 {
            break;
        }
        let before = response.inline_data.len();
        response.inline_data.retain(|data| {
            if remaining > 0 && image(data) {
                remaining -= 1;
                false
            } else {
                true
            }
        });
        let omitted = before - response.inline_data.len();
        if let Some(object) = response.response.as_object_mut() {
            object.insert("omitted_screenshots".into(), json!(omitted));
        }
    }
    request.previous_response_id = None;
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
    fn hides_only_bound_and_path_parameters() {
        let mut hidden = 0;
        for definition in crate::computer::catalog().as_array().unwrap() {
            let original = &definition["inputSchema"];
            let mut projected = parameters(definition);
            for name in crate::computer::HIDDEN {
                assert!(projected["properties"].get(name).is_none());
                assert!(
                    projected["required"]
                        .as_array()
                        .is_none_or(|required| !required.iter().any(|value| value == name))
                );
                if let Some(schema) = original["properties"].get(name) {
                    projected["properties"][name] = schema.clone();
                    hidden += 1;
                }
            }
            if let Some(required) = original.get("required") {
                projected["required"] = required.clone();
            }
            assert_eq!(&projected, original, "{}", definition["name"]);
        }
        assert!(hidden > 0);
    }

    mod screenshots {
        use super::*;
        use adk_core::{Content, FunctionResponseData, InlineDataPart, LlmRequest, Part};

        fn response(name: &str, images: usize) -> Part {
            let mut data = FunctionResponseData::new(name, json!({"isError":false}));
            data.inline_data = (0..images)
                .map(|index| InlineDataPart {
                    mime_type: "image/png".into(),
                    data: vec![index as u8],
                    uri: None,
                    annotations: None,
                })
                .collect();
            Part::FunctionResponse {
                function_response: data,
                id: None,
                annotations: None,
            }
        }

        fn request(parts: Vec<Part>) -> LlmRequest {
            let mut content = Content::new("user");
            content.parts = parts;
            let mut request = LlmRequest::new("model", vec![content]);
            request.previous_response_id = Some("previous".into());
            request
        }

        fn images(request: &LlmRequest) -> Vec<(String, usize)> {
            request.contents[0]
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::FunctionResponse {
                        function_response, ..
                    } => Some((
                        function_response.name.clone(),
                        function_response.inline_data.len(),
                    )),
                    _ => None,
                })
                .collect()
        }

        #[test]
        fn keeps_recent_until_batch() {
            let tools = vec!["get_window_state".to_owned()];
            let parts = (0..7).map(|_| response("get_window_state", 1)).collect();
            let mut request = request(parts);
            omit(&mut request, &tools);
            assert!(images(&request).iter().all(|(_, count)| *count == 1));
            assert_eq!(request.previous_response_id.as_deref(), Some("previous"));
        }

        #[test]
        fn omits_oldest_batch() {
            let tools = vec!["get_window_state".to_owned()];
            let mut parts: Vec<_> = (0..8).map(|_| response("get_window_state", 1)).collect();
            parts.insert(0, response("view_image", 1));
            let mut request = request(parts);
            omit(&mut request, &tools);
            let counts: Vec<_> = images(&request)
                .into_iter()
                .map(|(_, count)| count)
                .collect();
            assert_eq!(counts, [1, 0, 0, 0, 0, 0, 1, 1, 1]);
            let Part::FunctionResponse {
                function_response, ..
            } = &request.contents[0].parts[1]
            else {
                unreachable!()
            };
            assert_eq!(function_response.response["omitted_screenshots"], 1);
            assert!(request.previous_response_id.is_none());
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
