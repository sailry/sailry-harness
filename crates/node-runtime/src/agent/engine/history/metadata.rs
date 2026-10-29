use super::*;

pub(super) fn capture(source: &gdk::MessageMetadata, created: i64) -> Metadata {
    let inference = source.inference.as_ref().map(|inference| Inference {
        provider: inference.provider.clone(),
        requested_model: inference.requested_model.clone(),
        resolved_model: inference.resolved_model.clone(),
        provider_session: inference.provider_session_id.clone(),
    });
    let notes = OperationNotes {
        empty_response: source
            .operation_note("inference", "empty_response")
            .is_some(),
        delivered_subagent: source
            .operation_note("foreground_subagent", "delivered")
            .and_then(Value::as_str)
            .map(str::to_owned),
        cancelled_subagents: source
            .operation_note("foreground_subagent", "cancelled")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    };
    Metadata {
        runtime: Some(Runtime {
            created: Some(created),
            after: source
                .operation_note("tool_pair_compaction", "after")
                .and_then(Value::as_str)
                .map(str::to_owned),
            user_visible: source.user_visible,
            agent_visible: source.agent_visible,
            output_limit_reached: source.output_token_limit_reached,
            steer: source.steer,
            turn_context: source.turn_context,
            inference,
            notes: (notes.empty_response
                || notes.delivered_subagent.is_some()
                || !notes.cancelled_subagents.is_empty())
            .then_some(notes),
            logs: source.operation_logs.clone(),
        }),
        execution: source.usage.as_deref().map(|usage| Execution {
            timing: Some(Timing {
                elapsed_us: usage.elapsed_ms.map(|value| value.saturating_mul(1000)),
                first_token_us: usage
                    .time_to_first_token_ms
                    .map(|value| value.saturating_mul(1000)),
            }),
            ..Execution::default()
        }),
        ..Metadata::default()
    }
}

pub(super) fn usage(source: &gdk::MessageUsage) -> Usage {
    let count = |value: Option<i32>| value.and_then(|value| u64::try_from(value).ok());
    Usage {
        input: count(source.input_tokens),
        output: count(source.output_tokens),
        total: count(source.total_tokens),
        cache_read: count(source.cache_read_tokens),
        cache_write: count(source.cache_write_tokens),
        cost: source.cost,
        ..Usage::default()
    }
}

pub(super) fn project(record: &Record) -> gdk::MessageMetadata {
    let mut output = gdk::MessageMetadata::default();
    if let Some(runtime) = &record.metadata.runtime {
        if let Some(after) = &runtime.after {
            output.set_operation_note("tool_pair_compaction", "after", json!(after));
        }
        output.user_visible = runtime.user_visible;
        output.agent_visible = runtime.agent_visible;
        output.output_token_limit_reached = runtime.output_limit_reached;
        output.steer = runtime.steer;
        output.turn_context = runtime.turn_context;
        output.operation_logs = runtime.logs.clone();
        output.inference = runtime
            .inference
            .as_ref()
            .map(|inference| gdk::InferenceMetadata {
                provider: inference.provider.clone(),
                requested_model: inference.requested_model.clone(),
                resolved_model: inference.resolved_model.clone(),
                provider_session_id: inference.provider_session.clone(),
            });
        if let Some(notes) = &runtime.notes {
            if notes.empty_response {
                output.set_operation_note("inference", "empty_response", json!(true));
            }
            if let Some(id) = &notes.delivered_subagent {
                output.set_operation_note("foreground_subagent", "delivered", json!(id));
            }
            if !notes.cancelled_subagents.is_empty() {
                output.set_operation_note(
                    "foreground_subagent",
                    "cancelled",
                    json!(notes.cancelled_subagents),
                );
            }
        }
    }
    if matches!(record.kind, Kind::Compaction(_)) {
        output.user_visible = false;
        output.agent_visible = true;
    }
    if let Some(usage) = record.usage() {
        let count = |value: Option<u64>| value.and_then(|value| i32::try_from(value).ok());
        let timing = record
            .metadata
            .execution
            .as_ref()
            .and_then(|execution| execution.timing.as_ref());
        output.usage = Some(Box::new(gdk::MessageUsage {
            input_tokens: count(usage.input),
            output_tokens: count(usage.output),
            total_tokens: count(usage.total),
            cache_read_tokens: count(usage.cache_read),
            cache_write_tokens: count(usage.cache_write),
            cost: usage.cost,
            elapsed_ms: timing
                .and_then(|timing| timing.elapsed_us)
                .map(|value| value / 1000),
            time_to_first_token_ms: timing
                .and_then(|timing| timing.first_token_us)
                .map(|value| value / 1000),
            is_compaction: matches!(record.kind, Kind::Compaction(_))
                || record
                    .metadata
                    .execution
                    .as_ref()
                    .is_some_and(|execution| execution.purpose == Some(Purpose::Compaction)),
            ..gdk::MessageUsage::default()
        }));
    }
    output
}
