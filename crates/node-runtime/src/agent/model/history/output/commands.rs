use super::*;
pub(super) use crate::process::output::model::normalize;

const STREAM_EDGE: usize = 8 * 1024;

pub(super) fn id(value: &Value) -> Option<&str> {
    if let Some(id) = value.pointer("/sailry_command/id").and_then(Value::as_str) {
        return Some(id);
    }
    matches!(
        value["kind"].as_str(),
        Some("command_result" | "command_output")
    )
    .then(|| {
        value
            .pointer("/data/info/id")
            .or_else(|| value.pointer("/info/id"))
    })
    .flatten()
    .and_then(Value::as_str)
}

fn continuation(id: Option<&str>, stream: &str, offset: u64) -> String {
    let persisted = "read_tool_output with output, offset and limit for the persisted result JSON";
    id.map_or_else(|| persisted.into(), |id| format!("read_command id={id} stream={stream} offset={offset} with limit; use {persisted} if retained output is unavailable"))
}

pub(super) fn project(
    original: &Value,
    source: &Value,
    arguments: Option<&Value>,
) -> Option<Value> {
    let mut changed = false;
    let mut result = original.clone();
    let wrapped = original.get("data").is_some();
    let command = arguments
        .and_then(|arguments| arguments["command"].as_str())
        .or_else(|| {
            original
                .pointer(if wrapped {
                    "/data/info/command"
                } else {
                    "/info/command"
                })
                .and_then(Value::as_str)
        });
    for stream in ["stdout", "stderr"] {
        let data = if wrapped {
            result.get_mut("data")?
        } else {
            &mut result
        };
        let preview = &original["sailry_command"][stream];
        if let (Some(head), Some(tail), Some(total), Some(head_bytes), Some(tail_offset)) = (
            preview["head"].as_str(),
            preview["tail"].as_str(),
            preview["total_bytes"].as_u64(),
            preview["head_bytes"].as_u64(),
            preview["tail_offset"].as_u64(),
        ) {
            let omitted = tail_offset.saturating_sub(head_bytes);
            let hint = continuation(id(original), stream, head_bytes);
            let retained = preview["retained_bytes"].as_u64().unwrap_or(0);
            data[stream] = json!({
                "text":format!("{}\n[... {omitted} of {total} raw bytes omitted; {hint}; tail starts at offset={tail_offset}; raw retention ends at offset={retained} ...]\n{}", normalize(head), normalize(tail)),
                "truncated":true,"invalid_utf8":preview["invalid_utf8"],
                "total_bytes":total,"omitted_bytes":omitted,"head_bytes":head_bytes,
                "tail_offset":tail_offset,"retained_bytes":retained,
            });
            changed = true;
            continue;
        }
        let Some(text) = data[stream]["text"].as_str() else {
            continue;
        };
        let normalized = normalize(text);
        let reduced = command
            .and_then(|command| crate::process::output::model::cargo(command, &normalized))
            .filter(|text| text.len() < normalized.len())
            .unwrap_or(normalized);
        let replacement = if reduced.len() > STREAM_EDGE * 2 {
            // Measure offsets in the raw capture before terminal normalization or folding.
            let (head, tail) =
                crate::process::output::edges(text.as_bytes(), STREAM_EDGE, STREAM_EDGE);
            let omitted = tail - head;
            let hint = continuation(id(original), stream, head as u64);
            let total = text.len();
            let complete = data[stream]["truncated"] == false;
            let projected = format!(
                "{}\n[... {omitted} of {total} captured raw bytes omitted; {hint}; tail starts at offset={tail} ...]\n{}",
                normalize(&text[..head]),
                normalize(&text[tail..])
            );
            data[stream]["truncated"] = Value::Bool(true);
            data[stream]["omitted_bytes"] = json!(omitted);
            data[stream]["captured_bytes"] = json!(total);
            if complete {
                data[stream]["total_bytes"] = json!(total);
            }
            data[stream]["head_bytes"] = json!(head);
            data[stream]["tail_offset"] = json!(tail);
            Some(projected)
        } else {
            (reduced.len() < text.len()).then_some(reduced)
        };
        if let Some(text) = replacement {
            data[stream]["text"] = json!(text);
            changed = true;
        }
    }
    if changed {
        if let Some(id) = id(original) {
            result["sailry_command"] = json!({"id":id});
        }
        result["output"] = source.clone();
    }
    changed.then_some(result)
}
