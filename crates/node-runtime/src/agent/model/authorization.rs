//! Refreshes inline provider grants at each ADK generation, including tool continuations.
use super::*;
use crate::providers::login::Grant;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, Part};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use sailry_protocol::SessionId;
use serde_json::{Value, json};

pub(super) struct Model {
    pub(super) ingress: Arc<Ingress>,
    pub(super) provider: Provider,
    pub(super) config: SessionConfig,
    pub(super) session: SessionId,
    pub(super) delegated: bool,
    pub(super) effort: Effort,
    pub(super) stop: CancellationToken,
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        &self.config.model
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> Result<LlmResponseStream, AdkError> {
        let reference = self.config.credential.clone().ok_or_else(|| {
            failure(Fault::new(
                ErrorCode::NotConfigured,
                "provider sign-in is required",
            ))
        })?;
        let options = crate::providers::login::effective(&self.provider).map_err(failure)?;
        let grant = self
            .ingress
            .authorization(
                reference,
                self.provider.id,
                self.provider.authentication,
                &options,
                &self.stop,
            )
            .await
            .map_err(failure)?;
        let mut provider = self.provider.clone();
        provider.endpoint = grant.endpoint().to_owned();
        let base = grant.headers(&options).map_err(failure)?;
        let (access, adapter, stream) = match grant {
            Grant::ChatGpt { access, .. } => {
                let session =
                    HeaderValue::from_str(&self.session.to_string()).map_err(|_| invalid())?;
                let adapter: RequestAdapter = Arc::new(move |body, headers| {
                    headers.extend(base.clone());
                    headers.insert("session_id", session.clone());
                    chatgpt(body)
                });
                (access, adapter, true)
            }
            Grant::Copilot { access, .. } => {
                let initiator = if !self.delegated
                    && request.contents.last().is_some_and(|content| {
                        content.role == "user"
                            && !content
                                .parts
                                .iter()
                                .any(|part| matches!(part, Part::FunctionResponse { .. }))
                    }) {
                    "user"
                } else {
                    "agent"
                };
                let vision = request.contents.iter().flat_map(|content| &content.parts).any(|part| {
                    matches!(part, Part::InlineData { mime_type, .. } | Part::FileData { mime_type, .. } if mime_type.starts_with("image/"))
                });
                let adapter: RequestAdapter = Arc::new(move |_, headers| {
                    headers.extend(base.clone());
                    copilot(headers, initiator, vision)
                });
                (access, adapter, stream)
            }
        };
        // Test-only routing never changes persisted endpoints or production validation.
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = &self.ingress.authorization_endpoint {
            provider.endpoint = endpoint.clone();
        }
        let model = native(
            &provider,
            &self.config,
            access.expose(),
            self.effort,
            Some(adapter),
        )
        .map_err(failure)?;
        model.generate_content(request, stream).await
    }
}

fn chatgpt(body: &mut Value) -> Result<(), AdkError> {
    let fields = body.as_object_mut().ok_or_else(invalid)?;
    // This endpoint only accepts streamed, stateless Responses requests.
    if fields.get("stream") != Some(&Value::Bool(true)) {
        return Err(invalid());
    }
    fields.insert("store".into(), Value::Bool(false));
    fields
        .entry("instructions")
        .or_insert_with(|| Value::String(String::new()));
    let include = fields
        .entry("include")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(invalid)?;
    if !include
        .iter()
        .any(|value| value == "reasoning.encrypted_content")
    {
        include.push(json!("reasoning.encrypted_content"));
    }
    for key in [
        "max_output_tokens",
        "temperature",
        "top_p",
        "background",
        "metadata",
        "parallel_tool_calls",
        "service_tier",
        "text",
        "user",
    ] {
        fields.remove(key);
    }
    Ok(())
}

fn copilot(headers: &mut HeaderMap, initiator: &'static str, vision: bool) -> Result<(), AdkError> {
    for (name, value) in [
        ("openai-intent", "conversation-agent"),
        ("x-initiator", initiator),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers.insert(
        "x-request-id",
        HeaderValue::from_str(&sailry_protocol::RequestId::new().to_string())
            .map_err(|_| invalid())?,
    );
    if vision {
        headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
    }
    Ok(())
}

fn invalid() -> AdkError {
    AdkError::model("authorization request is invalid")
}
fn failure(fault: Fault) -> AdkError {
    AdkError::model("provider authorization failed").with_source(fault)
}
