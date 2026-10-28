use super::wire;
use super::*;

pub(super) fn capture(response: &wire::Response) -> Message {
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

pub(super) fn usage(value: &wire::Usage) -> Usage {
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
