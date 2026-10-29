use super::*;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

pub(super) fn capture(event: &Event) -> Result<Metadata, Fault> {
    let fields = &event.metadata;
    let decode = |key: &str| -> Result<Option<Value>, Fault> {
        fields
            .get(key)
            .map(|value| serde_json::from_str(value).map_err(storage_error))
            .transpose()
    };
    let input = Input {
        turn: fields
            .get("sailry.context.input_turn")
            .map(|value| value.parse().map_err(storage_error))
            .transpose()?,
        source: fields
            .get("sailry.context.input_source")
            .map(|value| serde_json::from_value(json!(value)).map_err(storage_error))
            .transpose()?,
        text_part: parse(fields, "sailry.context.input_part")?,
        capability_part: parse(fields, "sailry.context.capability_part")?,
        notice_part: fields.contains_key("sailry.context.notice_ids").then(|| {
            usize::from(
                event
                    .content()
                    .and_then(|content| content.parts.first())
                    .is_some_and(|part| matches!(part, Part::ConfigurationUpdate { .. })),
            )
        }),
        notice_ids: parse(fields, "sailry.context.notice_ids")?.unwrap_or_default(),
        references: parse(fields, "sailry_references")?.unwrap_or_default(),
    };
    let presentation = Presentation {
        groupings: parse(fields, "dev.sailry.tool_grouping")?.unwrap_or_default(),
        displays: parse(fields, "dev.sailry.tool_display")?.unwrap_or_default(),
        tools: parse(fields, "dev.sailry.tool_presentation")?.unwrap_or_default(),
        failures: parse(fields, "dev.sailry.tool_failures")?.unwrap_or_default(),
    };
    let timing = event
        .response
        .provider_metadata
        .as_ref()
        .and_then(|value| value.get("sailry_timing"))
        .map(|value| Timing {
            elapsed_us: value["elapsed_us"].as_u64(),
            first_token_us: value["first_token_us"].as_u64(),
        });
    let execution = Execution {
        purpose: fields
            .get("sailry_model_purpose")
            .map(|value| serde_json::from_value(json!(value)).map_err(storage_error))
            .transpose()?,
        media: fields
            .get("sailry_media")
            .map(|value| serde_json::from_value(json!(value)).map_err(storage_error))
            .transpose()?,
        timing,
        observation: parse(fields, "sailry_model_observation")?,
        phases: parse(fields, "sailry_execution_phases")?,
        retry: parse(fields, "sailry_retry")?,
        tool_started: parse(fields, "adk_tool_started")?,
        context_tokens: decode("sailry_context_projection")?
            .and_then(|value| value["tokens"].as_u64()),
    };
    let context = context(event)?;
    let mut maintenance = Maintenance {
        latest_instruction: fields.get("sailry.context.latest_instruction").cloned(),
        failure: parse(fields, "sailry_compaction_failure")?,
    };
    if let Some(code) = fields.get("sailry_operation_warning") {
        if event.author != "system" || code != "context_summary_failed" {
            return Err(Fault::invalid("invalid operation warning metadata"));
        }
        maintenance.failure.get_or_insert_with(|| {
            use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};
            Failure {
                source: Source::Automatic,
                stage: Stage::Unknown,
                reason: Reason::Unknown,
                continued: true,
                status: None,
                cause: None,
            }
        });
    }
    if maintenance.failure.is_some() && event.author != "system" {
        return Err(Fault::invalid("invalid compaction failure author"));
    }
    let visible = visible(event)?;
    let runtime = Runtime {
        user_visible: visible,
        agent_visible: visible,
        output_limit_reached: matches!(event.response.finish_reason.as_deref(), Some("MaxTokens")),
        turn_context: context.captured,
        ..Runtime::default()
    };
    Ok(Metadata {
        input: nonempty(input)?,
        presentation: nonempty(presentation)?,
        execution: nonempty(execution)?,
        context: nonempty(context)?,
        state: nonempty(extension_state(event))?,
        compaction: nonempty(maintenance)?,
        runtime: Some(runtime),
    })
}

fn extension_state(event: &Event) -> StateChanges {
    let mut state = StateChanges::default();
    for (key, value) in &event.actions.state_delta {
        if key.starts_with("temp:") || context_key(key) {
            continue;
        }
        if let Some(key) = key.strip_prefix("app:") {
            state.application.insert(key.into(), value.clone());
        } else if let Some(key) = key.strip_prefix("user:") {
            state.user.insert(key.into(), value.clone());
        } else {
            state.session.insert(key.clone(), value.clone());
        }
    }
    state
}

fn context_key(key: &str) -> bool {
    key.starts_with("sailry.context.guide:")
        || matches!(
            key,
            "sailry.session_title"
                | "sailry.context.policy"
                | "sailry.context.environment"
                | "sailry.context.personal"
                | "sailry.context.loaded_tools"
                | "sailry.context.masked_outputs"
        )
}

fn context(event: &Event) -> Result<Context, Fault> {
    let fields = &event.actions.state_delta;
    let string = |key: &str| {
        fields
            .get(key)
            .map(|value| serde_json::from_value(value.clone()).map_err(storage_error))
            .transpose()
    };
    let mut guides = std::collections::BTreeMap::new();
    for (key, value) in fields
        .iter()
        .filter(|(key, _)| key.starts_with("sailry.context.guide:"))
    {
        guides.insert(
            key.clone(),
            serde_json::from_value(value.clone()).map_err(storage_error)?,
        );
    }
    // The former masked-output cache belonged to the retired ADK loop.
    Ok(Context {
        captured: event
            .metadata
            .get("sailry.context")
            .is_some_and(|value| value == "context"),
        title: string("sailry.session_title")?,
        policy: string("sailry.context.policy")?,
        environment: string("sailry.context.environment")?,
        personal: string("sailry.context.personal")?,
        guides,
        loaded_tools: fields
            .get("sailry.context.loaded_tools")
            .map(|value| serde_json::from_value(value.clone()).map_err(storage_error))
            .transpose()?,
    })
}

fn parse<T: DeserializeOwned>(
    fields: &HashMap<String, String>,
    key: &str,
) -> Result<Option<T>, Fault> {
    fields
        .get(key)
        .map(|value| serde_json::from_str(value).map_err(storage_error))
        .transpose()
}

fn nonempty<T: Serialize + Default>(value: T) -> Result<Option<T>, Fault> {
    let encoded = serde_json::to_value(&value).map_err(storage_error)?;
    let default = serde_json::to_value(T::default()).map_err(storage_error)?;
    Ok((encoded != default).then_some(value))
}

// This is the existing telemetry visibility predicate evaluated once at import.
// Unknown actions or metadata remain visible, as they did in history's SQL.
fn visible(event: &Event) -> Result<bool, Fault> {
    let fields = &event.metadata;
    if event.author != "system" || event.content().is_some() {
        return Ok(true);
    }
    let telemetry = [
        "sailry_model_observation",
        "sailry_execution_phases",
        "sailry_context_projection",
    ];
    if (event.response.usage_metadata.is_some()
        && !(fields
            .get("sailry_model_purpose")
            .is_some_and(|value| value == "compaction")
            && fields.contains_key(telemetry[0])))
        || event.response.error_code.is_some()
        || event.response.error_message.is_some()
        || event.response.turn_complete
        || event.response.interrupted
        || !event.long_running_tool_ids.is_empty()
        || !telemetry.iter().any(|key| fields.contains_key(*key))
        || fields
            .keys()
            .any(|key| !telemetry.contains(&key.as_str()) && key != "sailry_model_purpose")
        || event
            .actions
            .state_delta
            .keys()
            .any(|key| key != "sailry.context.masked_outputs")
        || !event.actions.artifact_delta.is_empty()
    {
        return Ok(true);
    }
    Ok(event.actions.compaction.is_some()
        || event.actions.other.iter().any(|(key, value)| {
            !(value.is_null()
                || (key == "workflow_positions"
                    && value.as_object().is_some_and(|value| value.is_empty()))
                || (matches!(
                    key.as_str(),
                    "skip_summarization" | "escalate" | "input_screened"
                ) && value == &Value::Bool(false)))
        }))
}
