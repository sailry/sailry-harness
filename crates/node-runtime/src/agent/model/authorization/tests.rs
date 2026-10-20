use super::*;
use adk_core::{ErrorCategory, ErrorComponent};
use futures::StreamExt;

mod live;

struct Failure {
    message: String,
    opening: bool,
    adapter: adk_model::ErrorAdapter,
}

impl Failure {
    fn error(&self) -> AdkError {
        let mut error = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::InvalidInput,
            "model.openai_responses.invalid_request",
            "OpenAI Responses API request failed",
        )
        .with_upstream_status(400);
        let details = adk_model::ProviderErrorDetails {
            message: Some(self.message.clone()),
            code: Some(self.message.clone()),
            kind: Some(self.message.clone()),
            param: Some(self.message.clone()),
            request_id: Some(self.message.clone()),
            status: Some(400),
        };
        error
            .details
            .metadata
            .insert("provider_error_code".into(), json!(self.message));
        (self.adapter)(error, details)
    }
}

#[async_trait]
impl Llm for Failure {
    fn name(&self) -> &str {
        "authorization-error-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        let error = self.error();
        if self.opening {
            Err(error)
        } else {
            Ok(Box::pin(futures::stream::once(async move { Err(error) })))
        }
    }
}

#[tokio::test]
async fn captures_private_grant_fields_for_redaction() {
    use sailry_protocol::Secret;
    let mut headers = HeaderMap::new();
    let mut authorization = HeaderValue::from_static("Bearer fixture-header");
    authorization.set_sensitive(true);
    headers.insert("authorization", authorization);
    headers.insert(
        "chatgpt-account-id",
        HeaderValue::from_static("fixture-account"),
    );
    headers.insert("user-agent", HeaderValue::from_static("Public client"));
    for (kind, private) in [
        ("chat_gpt", vec!["fixture-refresh", "fixture-identity"]),
        (
            "chat_gpt_oauth",
            vec!["fixture-refresh", "fixture-identity"],
        ),
        ("claude_oauth", vec!["fixture-refresh"]),
        ("copilot", vec!["fixture-github"]),
    ] {
        let grant: Grant = serde_json::from_value(json!({
            "kind":kind, "access":"fixture-access", "refresh":"fixture-refresh",
            "id_token":"fixture-identity", "github":"fixture-github", "expires_at_ms":1,
            "earliest_refresh_at":null, "client_id":"fixture", "subject":"fixture",
            "account_id":"fixture-account", "email":null, "scopes":[],
            "renewal_pending":false, "endpoint":"https://example.invalid"
        }))
        .unwrap();
        let secrets = grant.secrets(&headers);
        let actual: std::collections::BTreeSet<_> = secrets
            .iter()
            .map(Secret::expose)
            .map(str::to_owned)
            .collect();
        let expected: std::collections::BTreeSet<_> = private
            .into_iter()
            .chain(["fixture-access", "Bearer fixture-header", "fixture-account"])
            .map(str::to_owned)
            .collect();
        assert_eq!(actual, expected);
        for opening in [false, true] {
            let model = super::super::errors::wrap(
                Arc::new(Failure {
                    message: format!(
                        "Provider error: {}",
                        actual.iter().cloned().collect::<Vec<_>>().join(" | ")
                    ),
                    opening,
                    adapter: super::super::errors::adapter(secrets.clone()),
                }),
                secrets.clone(),
            );
            let response = model
                .generate_content(LlmRequest::new(model.name(), vec![]), true)
                .await;
            let error = if opening {
                response.err().unwrap()
            } else {
                response.ok().unwrap().next().await.unwrap().err().unwrap()
            };
            let debug = format!("{error:?}");
            let problem = error.to_problem_json().to_string();
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Model,
                crate::logging::Phase::Failed,
            );
            super::super::diagnostics::cause(&mut entry, &error);
            assert_eq!(
                entry.cause_code,
                Some("model.openai_responses.invalid_request")
            );
            assert_eq!(entry.cause_component, Some("model"));
            assert_eq!(entry.cause_category, Some("invalid_input"));
            assert_eq!(entry.upstream_status, Some(400));
            assert_eq!(entry.parse_layer, None);
            assert_eq!(entry.parse_category, None);
            assert_eq!(entry.parse_cause, None);
            assert_eq!(entry.parse_field, None);
            assert_eq!(entry.parse_event, None);
            assert_eq!(entry.parse_line, None);
            assert_eq!(entry.parse_column, None);
            let fault = super::super::error(error);
            let projected = serde_json::to_string(&fault).unwrap();
            for private in &actual {
                assert!(!debug.contains(private));
                assert!(!problem.contains(private));
                assert!(!projected.contains(private));
                assert!(!fault.message.contains(private));
            }
            let (_, details) = fault.message.split_once("\n\n").unwrap();
            let details: Value = serde_json::from_str(details).unwrap();
            for field in ["message", "code", "type", "param", "request_id"] {
                assert!(
                    details[field]
                        .as_str()
                        .unwrap()
                        .starts_with("Provider error: ")
                );
                assert_eq!(
                    details[field]
                        .as_str()
                        .unwrap()
                        .matches("[redacted]")
                        .count(),
                    actual.len()
                );
            }
        }
    }
}

#[test]
fn uses_stateless_streaming() {
    let mut body = json!({
        "model": "selected-model",
        "stream": true,
        "store": true,
        "instructions": "Keep the full context",
        "input": [
            {"type":"message","role":"system","content":"Follow the task"},
            {"role":"user","content":"Continue"},
            {"type":"function_call","name":"read_file","call_id":"call_1","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"File contents"}
        ],
        "include": ["web_search_call.action.sources"],
        "parallel_tool_calls": true,
        "service_tier": "auto",
        "text": {"format":{"type":"text"}},
        "background": true,
        "conversation": "conversation_1",
        "max_output_tokens": 20,
        "max_tool_calls": 3,
        "metadata": {"key":"value"},
        "moderation": {},
        "multi_agent": {},
        "prompt": {},
        "prompt_cache_retention": "24h",
        "safety_identifier": "user_1",
        "temperature": 0.5,
        "top_logprobs": 2,
        "top_p": 0.9,
        "truncation": "auto",
        "user": "user_1",
        "previous_response_id": "response_1"
    });
    chatgpt(&mut body).unwrap();
    assert_eq!(body["model"], "selected-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "Keep the full context");
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(body["input"][2]["name"], "read_file");
    assert_eq!(body["input"][2]["namespace"], "sailry");
    assert_eq!(body["input"][3]["output"], "File contents");
    assert_eq!(
        body["include"],
        json!([
            "web_search_call.action.sources",
            "reasoning.encrypted_content"
        ])
    );
    assert_eq!(body["parallel_tool_calls"], true);
    assert_eq!(body["service_tier"], "auto");
    assert_eq!(body["text"], json!({"format":{"type":"text"}}));
    for key in [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        assert!(body.get(key).is_none(), "unexpected field: {key}");
    }
    chatgpt(&mut body).unwrap();
    assert_eq!(body["include"].as_array().unwrap().len(), 2);
}

#[test]
fn groups_local_tools() {
    let function = json!({
        "type":"function","name":"read_file","description":"Read a file",
        "parameters":{"type":"object","properties":{"path":{"type":"string"}}},
        "strict":false
    });
    let custom = json!({"type":"custom","name":"patch","description":"Apply a patch"});
    let web = json!({"type":"web_search","search_context_size":"medium"});
    let existing = json!({
        "type":"namespace","name":"external","description":"External tools",
        "tools":[{"type":"function","name":"lookup","parameters":{}}]
    });
    let mut body = json!({
        "stream":true,"input":[
            {"type":"custom_tool_call","name":"patch","call_id":"call_2","input":"patch"},
            {"type":"function_call","name":"lookup","namespace":"external","call_id":"call_3","arguments":"{}"}
        ],
        "tools":[function,custom,web,existing]
    });
    chatgpt(&mut body).unwrap();
    assert_eq!(body["tools"][0], web);
    assert_eq!(body["tools"][1], existing);
    assert_eq!(body["tools"][2]["type"], "namespace");
    assert_eq!(body["tools"][2]["name"], "sailry");
    assert_eq!(body["tools"][2]["tools"], json!([function, custom]));
    assert_eq!(body["input"][0]["namespace"], "sailry");
    assert_eq!(body["input"][1]["namespace"], "external");
    chatgpt(&mut body).unwrap();
    assert_eq!(body["tools"].as_array().unwrap().len(), 3);
}

#[test]
fn rejects_unsupported_requests() {
    for mut body in [
        json!({"stream":false,"input":[]}),
        json!({"stream":true,"input":"prompt"}),
        json!({"stream":true,"input":[],"include":false}),
    ] {
        assert!(chatgpt(&mut body).is_err());
    }
    for tool in [
        "image_generation",
        "file_search",
        "code_interpreter",
        "computer_use_preview",
        "mcp",
        "tool_search",
        "programmatic_tool_calling",
    ] {
        let mut body = json!({"stream":true,"input":[],"tools":[{"type":tool}]});
        assert!(chatgpt(&mut body).is_err(), "unsupported tool: {tool}");
    }
}
