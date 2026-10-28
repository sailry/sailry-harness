use super::*;
use adk_core::{CitationMetadata, CitationSource, LlmResponse, UsageMetadata};

pub(super) fn capture(response: &LlmResponse) -> Message {
    let fields = response.provider_metadata.as_ref();
    Message {
        role: if response.content.as_ref().is_some_and(|content| {
            content.role == "user" || content.role == "function" || content.role == "tool"
        }) {
            Role::User
        } else {
            Role::Assistant
        },
        part_count: response
            .content
            .as_ref()
            .map_or(0, |content| content.parts.len()),
        content: Vec::new(),
        model: response.model.clone(),
        usage: response.usage_metadata.as_ref().map(usage),
        finish: if response.interrupted {
            Some(Finish::Interrupted)
        } else if response.error_code.is_some() || response.error_message.is_some() {
            Some(Finish::Failed)
        } else if response.turn_complete {
            Some(Finish::Completed)
        } else {
            None
        },
        error: (response.error_code.is_some() || response.error_message.is_some()).then(|| {
            ResponseError {
                code: response.error_code.clone(),
                message: response.error_message.clone().unwrap_or_default(),
            }
        }),
        sources: Sources {
            citations: response
                .citation_metadata
                .as_ref()
                .into_iter()
                .flat_map(|metadata| &metadata.citation_sources)
                .map(|source| Citation {
                    uri: source.uri.clone(),
                    title: source.title.clone(),
                    start: source.start_index.map(i64::from),
                    end: source.end_index.map(i64::from),
                })
                .collect(),
            grounding: fields
                .and_then(|fields| fields["groundingChunks"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|chunk| {
                    Some(Grounding {
                        uri: chunk["web"]["uri"].as_str()?.into(),
                        title: chunk["web"]["title"].as_str().map(str::to_owned),
                    })
                })
                .collect(),
            search_suggestions: fields
                .and_then(|fields| fields["searchEntryPoint"]["renderedContent"].as_str())
                .map(str::to_owned),
        },
    }
}

pub(super) fn usage(value: &UsageMetadata) -> Usage {
    let count = |value| u64::try_from(value).ok();
    Usage {
        input: count(value.prompt_token_count),
        output: count(value.candidates_token_count),
        total: count(value.total_token_count),
        cache_read: value.cache_read_input_token_count.and_then(count),
        cache_write: value.cache_creation_input_token_count.and_then(count),
        reasoning: value.thinking_token_count.and_then(count),
        audio_input: value.audio_input_token_count.and_then(count),
        audio_output: value.audio_output_token_count.and_then(count),
        cost: value.cost,
        byok: value.is_byok,
        provider: value.provider_usage.clone(),
        builtins: value
            .builtin_tool_uses
            .iter()
            .map(|usage| BuiltinUsage {
                name: usage.kind.clone(),
                count: u64::from(usage.count),
            })
            .collect(),
    }
}

pub(super) fn restore(
    response: &mut LlmResponse,
    message: &Message,
    metadata: &Metadata,
) -> Result<(), Fault> {
    response.model = message.model.clone();
    response.usage_metadata = message.usage.as_ref().map(restore_usage).transpose()?;
    response.turn_complete = message.finish == Some(Finish::Completed);
    response.interrupted = message.finish == Some(Finish::Interrupted);
    if let Some(error) = &message.error {
        response.error_code = error.code.clone();
        response.error_message = Some(error.message.clone());
    }
    let mut fields = serde_json::Map::new();
    if let Some(timing) = metadata
        .execution
        .as_ref()
        .and_then(|execution| execution.timing.as_ref())
    {
        fields.insert(
            "sailry_timing".into(),
            serde_json::to_value(timing).map_err(storage_error)?,
        );
    }
    if metadata
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.output_limit_reached)
    {
        response.finish_reason = Some(adk_core::FinishReason::MaxTokens);
    }
    if !message.sources.grounding.is_empty() {
        fields.insert(
            "groundingChunks".into(),
            json!(
                message
                    .sources
                    .grounding
                    .iter()
                    .map(|source| json!({"web":{"uri":source.uri,"title":source.title}}))
                    .collect::<Vec<_>>()
            ),
        );
    }
    if let Some(suggestions) = &message.sources.search_suggestions {
        fields.insert(
            "searchEntryPoint".into(),
            json!({"renderedContent":suggestions}),
        );
    }
    if !fields.is_empty() {
        response.provider_metadata = Some(Value::Object(fields));
    }
    if !message.sources.citations.is_empty() {
        response.citation_metadata = Some(CitationMetadata {
            citation_sources: message
                .sources
                .citations
                .iter()
                .map(|source| CitationSource {
                    uri: source.uri.clone(),
                    title: source.title.clone(),
                    start_index: source.start.and_then(|value| i32::try_from(value).ok()),
                    end_index: source.end.and_then(|value| i32::try_from(value).ok()),
                    license: None,
                    publication_date: None,
                })
                .collect(),
        });
    }
    Ok(())
}

fn restore_usage(value: &Usage) -> Result<UsageMetadata, Fault> {
    let count = |value: u64| i32::try_from(value).map_err(storage_error);
    Ok(UsageMetadata {
        prompt_token_count: count(value.input.unwrap_or(0))?,
        candidates_token_count: count(value.output.unwrap_or(0))?,
        total_token_count: count(value.total.unwrap_or(0))?,
        cache_read_input_token_count: value.cache_read.map(count).transpose()?,
        cache_creation_input_token_count: value.cache_write.map(count).transpose()?,
        thinking_token_count: value.reasoning.map(count).transpose()?,
        audio_input_token_count: value.audio_input.map(count).transpose()?,
        audio_output_token_count: value.audio_output.map(count).transpose()?,
        cost: value.cost,
        is_byok: value.byok,
        provider_usage: value.provider.clone(),
        builtin_tool_uses: value
            .builtins
            .iter()
            .map(|usage| {
                Ok(adk_core::BuiltinToolUse {
                    kind: usage.name.clone(),
                    count: u32::try_from(usage.count).map_err(storage_error)?,
                })
            })
            .collect::<Result<_, Fault>>()?,
    })
}
