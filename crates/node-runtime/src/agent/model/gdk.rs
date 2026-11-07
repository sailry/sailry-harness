use super::types::*;
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_types::{
    base::{MessageStream, Provider},
    conversation::{
        message::{self as legacy, MessageContentBlock as Block},
        token_usage::{ProviderStats, ProviderUsage},
    },
    errors::ProviderError,
    model::ModelConfig,
    thinking::ThinkingEffort,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

pub(crate) fn provider(inner: Arc<dyn Model>, owner: String) -> Arc<dyn Provider> {
    Arc::new(Adapter { inner, owner })
}

pub(super) fn context(api: sailry_protocol::conversation::ModelApi, model: &str) -> usize {
    use sailry_protocol::conversation::ModelApi;
    if api == ModelApi::AzureAi {
        let resolve = |model: &str| {
            goose_provider_types::canonical::maybe_get_canonical_model("azure_foundry", model)
                .or_else(|| {
                    goose_provider_types::canonical::maybe_get_canonical_model(
                        "azure_foundry",
                        &model.to_ascii_lowercase(),
                    )
                })
        };
        let canonical = resolve(model).or_else(|| {
            let (base, effort) =
                goose_provider_types::formats::openai::extract_reasoning_effort(model);
            effort.and_then(|_| resolve(&base))
        });
        if let Some(canonical) = canonical {
            return canonical.limit.context;
        }
    }
    let provider = match api {
        ModelApi::Responses | ModelApi::ChatCompletions => "openai",
        ModelApi::Anthropic => "anthropic",
        ModelApi::Gemini => "google",
        ModelApi::AzureOpenAi => "azure_openai",
        ModelApi::AzureAi => "azure_foundry",
        ModelApi::DeepSeek => "custom_deepseek",
        ModelApi::OpenCodeGo => "opencode_go",
        ModelApi::OpenCodeZen => "opencode_zen",
        ModelApi::Plugin => "sailry_plugin",
    };
    goose_provider_types::context_limit::ContextLimitResolver::new(provider)
        .resolve_local(model, None)
}

pub(crate) fn config(source: &Config) -> ModelConfig {
    let mut config = ModelConfig::new(&source.model_name)
        .with_context_limit(source.context_limit)
        .with_max_tokens(
            source
                .max_tokens
                .and_then(|value| i32::try_from(value).ok()),
        );
    config.temperature = source.temperature.map(|value| value as f32);
    config.reasoning = source.reasoning;
    config.supports_vision = source.supports_vision;
    config.request_params = Some(source.request_params.clone().into_iter().collect());
    match source.thinking_effort {
        sailry_protocol::Effort::Default => {}
        sailry_protocol::Effort::Disabled => {
            config = config.with_thinking_effort(ThinkingEffort::Off)
        }
        sailry_protocol::Effort::Minimal | sailry_protocol::Effort::Low => {
            config = config.with_thinking_effort(ThinkingEffort::Low)
        }
        sailry_protocol::Effort::Medium => {
            config = config.with_thinking_effort(ThinkingEffort::Medium)
        }
        sailry_protocol::Effort::High => config = config.with_thinking_effort(ThinkingEffort::High),
        sailry_protocol::Effort::XHigh | sailry_protocol::Effort::Max => {
            config = config.with_thinking_effort(ThinkingEffort::Max)
        }
        sailry_protocol::Effort::Budget(tokens) => {
            config =
                config.with_merged_request_params([("budget_tokens".into(), json!(tokens))].into())
        }
    }
    config
        .request_params
        .get_or_insert_with(Default::default)
        .insert(
            "sailry_effort".into(),
            serde_json::to_value(source.thinking_effort).unwrap(),
        );
    config
}

pub(crate) fn request(
    config: &ModelConfig,
    system: &str,
    messages: &[legacy::Message],
    tools: &[rmcp::model::Tool],
) -> Result<Request, Error> {
    let mut params: serde_json::Map<_, _> = config
        .request_params
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let effort = params
        .remove("sailry_effort")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| match config.thinking_effort() {
            Some(ThinkingEffort::Off) => sailry_protocol::Effort::Disabled,
            Some(ThinkingEffort::Low) => sailry_protocol::Effort::Low,
            Some(ThinkingEffort::Medium) => sailry_protocol::Effort::Medium,
            Some(ThinkingEffort::High) => sailry_protocol::Effort::High,
            Some(ThinkingEffort::Max) => sailry_protocol::Effort::Max,
            None => sailry_protocol::Effort::Default,
        });
    params.retain(|key, _| !goose_provider_types::model::is_goose_internal_request_param(key));
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in config.request_headers.iter().flatten() {
        headers.insert(
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                Error::new(ErrorKind::InvalidRequest, "model header name is invalid")
            })?,
            reqwest::header::HeaderValue::from_str(value).map_err(|_| {
                Error::new(ErrorKind::InvalidRequest, "model header value is invalid")
            })?,
        );
    }
    Ok(Request {
        model: config.model_name.clone(),
        system: system.to_owned(),
        messages: messages.iter().map(from_message).collect(),
        tools: tools
            .iter()
            .map(|tool| Tool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(ToString::to_string),
                parameters: Value::Object(tool.input_schema.as_ref().clone()),
            })
            .collect(),
        max_tokens: config
            .max_tokens
            .and_then(|value| u64::try_from(value).ok()),
        temperature: config.temperature.map(f64::from),
        effort,
        params,
        headers,
        context_limit: config.context_limit,
        reasoning: config.reasoning,
    })
}

pub(crate) fn from_message(source: &legacy::Message) -> Message {
    Message {
        id: source.id.clone(),
        created: source.created,
        role: match source.role {
            rmcp::model::Role::User => Role::User,
            _ => Role::Assistant,
        },
        content: source
            .content
            .iter()
            .filter_map(|block| match block {
                Block::Text(text) => Some(Content::Text {
                    text: text.text.clone(),
                }),
                Block::Thinking(thinking) => Some(Content::Thinking {
                    text: thinking.thinking.clone(),
                    signature: Some(thinking.signature.clone()),
                }),
                Block::RedactedThinking(thinking) => Some(Content::RedactedThinking {
                    data: thinking.data.clone(),
                }),
                Block::Image(image) => Some(Content::Image {
                    media_type: image.mime_type.clone(),
                    data: if image.data.starts_with("https://") || image.data.starts_with("http://")
                    {
                        Image::Url {
                            url: image.data.clone(),
                        }
                    } else {
                        Image::Inline {
                            data_base64: image.data.clone(),
                        }
                    },
                    annotations: image
                        .annotations
                        .as_ref()
                        .and_then(|value| serde_json::to_value(value).ok()),
                }),
                Block::Document(document) => Some(Content::Document {
                    media_type: document.mime_type.clone(),
                    name: document.name.clone(),
                    data_base64: document.data.clone(),
                }),
                Block::ToolRequest(call) => {
                    let (name, arguments, error) = match &call.tool_call {
                        Ok(request) => (
                            request.name.to_string(),
                            Value::Object(request.arguments.clone().unwrap_or_default()),
                            None,
                        ),
                        Err(error) => (
                            String::new(),
                            Value::Null,
                            Some(ToolError {
                                code: error.code.0,
                                message: error.message.to_string(),
                                data: error.data.clone(),
                            }),
                        ),
                    };
                    Some(Content::ToolCall(ToolCall {
                        id: call.id.clone(),
                        name,
                        arguments,
                        error,
                        metadata: call.metadata.clone(),
                        tool_meta: call.tool_meta.clone(),
                    }))
                }
                Block::ToolResponse(result) => Some(Content::ToolResult(ToolResult {
                    id: result.id.clone(),
                    name: String::new(),
                    result: result.tool_result.clone(),
                    metadata: result.metadata.clone(),
                })),
                Block::Error(error) => {
                    let message = if error.kind == legacy::MessageErrorKind::Other
                        && error.message.starts_with("Network error: ")
                    {
                        error
                            .message
                            .strip_suffix("\n\nPlease resend your message to try again.")
                            .unwrap_or(&error.message)
                    } else {
                        &error.message
                    };
                    Some(Content::Error {
                        code: serde_json::to_value(error.kind)
                            .ok()
                            .and_then(|value| value.as_str().map(str::to_owned)),
                        message: message.to_owned(),
                    })
                }
                Block::SystemNotification(notification)
                    if notification.notification_type
                        == legacy::SystemNotificationType::CreditsExhausted =>
                {
                    Some(Content::Error {
                        code: Some("credits_exhausted".into()),
                        message: notification.msg.clone(),
                    })
                }
                _ => None,
            })
            .collect(),
        metadata: Metadata {
            user_visible: source.metadata.user_visible,
            agent_visible: source.metadata.agent_visible,
            inference: source
                .metadata
                .inference
                .as_ref()
                .map(|inference| Inference {
                    provider: inference.provider.clone(),
                    requested_model: inference.requested_model.clone(),
                    resolved_model: inference.resolved_model.clone(),
                    provider_session_id: inference.provider_session_id.clone(),
                }),
            output_token_limit_reached: source.metadata.output_token_limit_reached,
            steer: source.metadata.steer,
            turn_context: source.metadata.turn_context,
            compaction_summary: source.metadata.compaction_summary,
            usage: source.metadata.usage.as_deref().map(|usage| {
                Box::new(Usage {
                    input: count(usage.input_tokens),
                    output: count(usage.output_tokens),
                    total: count(usage.total_tokens),
                    cache_read: count(usage.cache_read_tokens),
                    cache_write: count(usage.cache_write_tokens),
                    cost: usage.cost,
                    elapsed_ms: usage.elapsed_ms,
                    time_to_first_token_ms: usage.time_to_first_token_ms,
                    is_compaction: usage.is_compaction,
                    ..Usage::default()
                })
            }),
            operation_logs: source.metadata.operation_logs.clone(),
            operations: source.metadata.operations.clone(),
        },
    }
}

pub(crate) fn to_message(source: &Message) -> legacy::Message {
    let mut message = match source.role {
        Role::User => legacy::Message::user(),
        Role::Assistant => legacy::Message::assistant(),
    };
    message.id = source.id.clone();
    message.created = source.created;
    for content in &source.content {
        let block = match content {
            Content::Text { text } => Block::text(text),
            Content::Thinking { text, signature } => {
                Block::thinking(text, signature.as_deref().unwrap_or_default())
            }
            Content::RedactedThinking { data } => Block::redacted_thinking(data),
            Content::Image {
                media_type,
                data,
                annotations,
            } => {
                let data = match data {
                    Image::Inline { data_base64 } => data_base64,
                    Image::Url { url } => url,
                };
                let mut block = Block::image(data, media_type);
                if let Block::Image(image) = &mut block {
                    image.annotations = annotations
                        .as_ref()
                        .and_then(|value| serde_json::from_value(value.clone()).ok());
                }
                block
            }
            Content::Document {
                media_type,
                name,
                data_base64,
            } => Block::document(data_base64, media_type, name.clone()),
            Content::ToolCall(call) => Block::ToolRequest(legacy::ToolRequest {
                id: call.id.clone(),
                tool_call: match &call.error {
                    Some(error) => Err(rmcp::model::ErrorData {
                        code: rmcp::model::ErrorCode(error.code),
                        message: error.message.clone().into(),
                        data: error.data.clone(),
                    }),
                    None => Ok(rmcp::model::CallToolRequestParams::new(call.name.clone())
                        .with_arguments(call.arguments.as_object().cloned().unwrap_or_default())),
                },
                metadata: call.metadata.clone(),
                tool_meta: call.tool_meta.clone(),
            }),
            Content::ToolResult(result) => Block::ToolResponse(legacy::ToolResponse {
                id: result.id.clone(),
                tool_result: result.result.clone(),
                metadata: result.metadata.clone(),
            }),
            Content::Error { code, message } => Block::error(
                code.as_ref()
                    .and_then(|code| serde_json::from_value(Value::String(code.clone())).ok())
                    .unwrap_or(legacy::MessageErrorKind::Other),
                message,
            ),
        };
        message.content.push(block);
    }
    message.metadata =
        legacy::MessageMetadata {
            user_visible: source.metadata.user_visible,
            agent_visible: source.metadata.agent_visible,
            inference: source.metadata.inference.as_ref().map(|inference| {
                legacy::InferenceMetadata {
                    provider: inference.provider.clone(),
                    requested_model: inference.requested_model.clone(),
                    resolved_model: inference.resolved_model.clone(),
                    provider_session_id: inference.provider_session_id.clone(),
                }
            }),
            output_token_limit_reached: source.metadata.output_token_limit_reached,
            steer: source.metadata.steer,
            turn_context: source.metadata.turn_context,
            compaction_summary: source.metadata.compaction_summary,
            usage: source.metadata.usage.as_deref().map(|usage| {
                Box::new(legacy::MessageUsage {
                    input_tokens: narrow(usage.input),
                    output_tokens: narrow(usage.output),
                    total_tokens: narrow(usage.total),
                    cache_read_tokens: narrow(usage.cache_read),
                    cache_write_tokens: narrow(usage.cache_write),
                    cost: usage.cost,
                    elapsed_ms: usage.elapsed_ms,
                    time_to_first_token_ms: usage.time_to_first_token_ms,
                    is_compaction: usage.is_compaction,
                    ..legacy::MessageUsage::default()
                })
            }),
            operation_logs: source.metadata.operation_logs.clone(),
            operations: source.metadata.operations.clone(),
        };
    message
}

pub(crate) fn from_usage(source: &ProviderUsage) -> Usage {
    Usage {
        model: Some(source.model.clone()),
        input: count(source.usage.input_tokens),
        output: count(source.usage.output_tokens),
        total: count(source.usage.total_tokens),
        cache_read: count(source.usage.cache_read_input_tokens),
        cache_write: count(source.usage.cache_write_input_tokens),
        reasoning: source
            .additional_data
            .as_ref()
            .and_then(|data| data.get("reasoning_tokens"))
            .and_then(Value::as_u64),
        provider: source.additional_data.clone().map(Value::Object),
        cost: source.cost,
        elapsed_ms: source.stats.as_ref().and_then(|stats| stats.elapsed_ms),
        time_to_first_token_ms: source
            .stats
            .as_ref()
            .and_then(|stats| stats.time_to_first_token_ms),
        ..Usage::default()
    }
}

pub(crate) fn to_usage(source: &Usage) -> ProviderUsage {
    let mut usage = ProviderUsage::new(
        source.model.clone().unwrap_or_default(),
        goose_provider_types::conversation::token_usage::Usage {
            input_tokens: narrow(source.input),
            output_tokens: narrow(source.output),
            total_tokens: narrow(source.total),
            cache_read_input_tokens: narrow(source.cache_read),
            cache_write_input_tokens: narrow(source.cache_write),
        },
    );
    usage.cost = source.cost;
    usage.stats = Some(ProviderStats {
        elapsed_ms: source.elapsed_ms,
        time_to_first_token_ms: source.time_to_first_token_ms,
        ..ProviderStats::default()
    });
    usage.additional_data = source.provider.as_ref().and_then(Value::as_object).cloned();
    if let Some(tokens) = source.reasoning {
        usage
            .additional_data
            .get_or_insert_with(Default::default)
            .insert("reasoning_tokens".into(), tokens.into());
    }
    usage
}

pub(crate) fn error(source: ProviderError) -> Error {
    let kind = match &source {
        ProviderError::NotConfigured => ErrorKind::NotConfigured,
        ProviderError::Authentication(_) => ErrorKind::Authentication,
        ProviderError::ContextLengthExceeded(_) => ErrorKind::ContextOverflow,
        ProviderError::RateLimitExceeded { .. } => ErrorKind::RateLimited,
        ProviderError::CreditsExhausted { .. } => ErrorKind::Quota,
        ProviderError::NetworkError(message) if message.starts_with("Stream decode error: ") => {
            ErrorKind::InvalidResponse
        }
        ProviderError::NetworkError(_) => ErrorKind::Network,
        ProviderError::ServerError(_) => ErrorKind::Unavailable,
        ProviderError::InvalidValue(_) => ErrorKind::InvalidRequest,
        ProviderError::NotImplemented(_) => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    };
    let retry_after = match &source {
        ProviderError::RateLimitExceeded { retry_delay, .. } => *retry_delay,
        _ => None,
    };
    Error {
        kind,
        message: source.to_string(),
        retry_after,
    }
}

fn legacy_error(source: Error) -> ProviderError {
    match source.kind {
        ErrorKind::NotConfigured => ProviderError::NotConfigured,
        ErrorKind::Authentication => ProviderError::Authentication(source.message),
        ErrorKind::ContextOverflow => ProviderError::ContextLengthExceeded(source.message),
        ErrorKind::RateLimited => ProviderError::RateLimitExceeded {
            details: source.message,
            retry_delay: source.retry_after,
        },
        ErrorKind::Quota => ProviderError::CreditsExhausted {
            details: source.message,
            top_up_url: None,
        },
        ErrorKind::Network => ProviderError::NetworkError(source.message),
        ErrorKind::Unavailable => ProviderError::ServerError(source.message),
        ErrorKind::InvalidRequest => ProviderError::InvalidValue(source.message),
        ErrorKind::InvalidResponse => ProviderError::RequestFailed(source.message),
        ErrorKind::Unsupported => ProviderError::NotImplemented(source.message),
        ErrorKind::Cancelled => ProviderError::ExecutionError(source.message),
        ErrorKind::Other => ProviderError::RequestFailed(source.message),
    }
}

fn count(value: Option<i32>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}
fn narrow(value: Option<u64>) -> Option<i32> {
    value.and_then(|value| i32::try_from(value).ok())
}

struct Adapter {
    inner: Arc<dyn Model>,
    owner: String,
}

#[async_trait]
impl Provider for Adapter {
    fn get_name(&self) -> &str {
        &self.owner
    }

    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[legacy::Message],
        tools: &[rmcp::model::Tool],
    ) -> Result<MessageStream, ProviderError> {
        let request = request(config, system, messages, tools).map_err(legacy_error)?;
        let stream = self
            .inner
            .call(request, CancellationToken::new())
            .await
            .map_err(legacy_error)?;
        let state = Streaming {
            stream,
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            created: chrono::Utc::now().timestamp(),
            pending: VecDeque::new(),
            reasoning: BTreeMap::new(),
            usage: Usage::default(),
            done: false,
        };
        Ok(Box::pin(futures::stream::unfold(
            state,
            |mut state| async move {
                loop {
                    if let Some(output) = state.pending.pop_front() {
                        return Some((output, state));
                    }
                    if state.done {
                        return None;
                    }
                    match state.stream.next().await {
                        Some(Ok(event)) => state.receive(event),
                        Some(Err(error)) => {
                            state.flush();
                            state.pending.push_back(Err(legacy_error(error)));
                            state.done = true;
                        }
                        None => {
                            state.flush();
                            state.done = true;
                        }
                    }
                }
            },
        )))
    }
}

struct Streaming {
    stream: ModelStream,
    id: String,
    created: i64,
    pending: VecDeque<Result<(Option<legacy::Message>, Option<ProviderUsage>), ProviderError>>,
    reasoning: BTreeMap<usize, (String, String)>,
    usage: Usage,
    done: bool,
}

impl Streaming {
    fn emit(&mut self, mut message: legacy::Message, usage: Option<ProviderUsage>) {
        message.id = Some(self.id.clone());
        message.created = self.created;
        self.pending.push_back(Ok((Some(message), usage)));
    }

    fn flush(&mut self) {
        for (_, (text, signature)) in std::mem::take(&mut self.reasoning) {
            self.emit(
                legacy::Message::assistant().with_thinking(text, signature),
                None,
            );
        }
    }

    fn receive(&mut self, event: Event) {
        match event {
            Event::TextDelta { text, .. } => {
                self.flush();
                self.emit(legacy::Message::assistant().with_text(text), None);
            }
            Event::ReasoningDelta {
                index,
                text,
                signature,
            } => {
                let block = self.reasoning.entry(index).or_default();
                if let Some(text) = text {
                    block.0.push_str(&text);
                }
                if let Some(signature) = signature {
                    block.1.push_str(&signature);
                    if !block.1.is_empty() {
                        let (text, signature) = self.reasoning.remove(&index).unwrap();
                        self.emit(
                            legacy::Message::assistant().with_thinking(text, signature),
                            None,
                        );
                    }
                }
            }
            Event::RedactedThinking { data, .. } => {
                self.flush();
                self.emit(
                    legacy::Message::assistant().with_redacted_thinking(data),
                    None,
                );
            }
            Event::ToolCallDelta { .. } => {}
            Event::ToolCall { call, .. } => {
                self.flush();
                self.emit(
                    to_message(&Message {
                        content: vec![Content::ToolCall(call)],
                        ..Message::default()
                    }),
                    None,
                );
            }
            Event::Usage(usage) => {
                self.usage = usage.clone();
                self.pending.push_back(Ok((None, Some(to_usage(&usage)))));
            }
            Event::Finish(reason) => {
                self.flush();
                let mut usage = to_usage(&self.usage);
                usage.finish_reasons = Some(vec![match reason {
                    Finish::Stop => "stop".into(),
                    Finish::ToolCalls => "tool_calls".into(),
                    Finish::Length => "length".into(),
                    Finish::ContentFilter => "content_filter".into(),
                    Finish::Other(reason) => reason,
                }]);
                let mut message = legacy::Message::assistant();
                message.metadata.output_token_limit_reached =
                    matches!(usage.finish_reasons.as_deref(), Some([reason]) if reason == "length");
                self.emit(message, Some(usage));
                self.done = true;
            }
        }
    }
}

#[cfg(test)]
mod tests;
