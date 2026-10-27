use super::*;
pub(super) use crate::process::output::model::normalize;

const STREAM_EDGE: usize = 8 * 1024;

pub(super) fn id(value: &Value) -> Option<&str> {
    value
        .pointer("/sailry_command/id")
        .or_else(|| value.pointer("/data/info/id"))
        .or_else(|| value.pointer("/info/id"))
        .and_then(Value::as_str)
}

pub(super) fn project(
    original: &Value,
    source: &Value,
    arguments: Option<&Value>,
) -> Option<Value> {
    let mut changed = false;
    let mut result = original.clone();
    let wrapped = original.get("data").is_some();
    if let Some(retained) = original.get("sailry_command") {
        for stream in ["stdout", "stderr"] {
            if retained[stream].is_object() {
                let data = if wrapped {
                    result.get_mut("data")?
                } else {
                    &mut result
                };
                data[stream] = retained[stream].clone();
            }
        }
        result["sailry_command"] = json!({"id":retained["id"]});
        changed = true;
    }
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
        let Some(text) = data[stream]["text"].as_str() else {
            continue;
        };
        let normalized = normalize(text);
        let reduced = command
            .and_then(|command| crate::process::output::model::cargo(command, &normalized))
            .filter(|text| text.len() < normalized.len())
            .unwrap_or(normalized);
        let text = if reduced.len() > STREAM_EDGE * 2 {
            let (head, tail) = crate::process::output::edges(&reduced, STREAM_EDGE, STREAM_EDGE);
            let omitted = reduced.len() - head.len() - tail.len();
            let continuation = id(original).map_or_else(
                || "read_tool_output with output, offset=0 and limit for the original result".into(),
                |id| format!("read_command id={id} stream={stream} offset=0 with limit for retained output"),
            );
            let text = format!(
                "{head}\n[... {omitted} bytes of this preview omitted; use {continuation} ...]\n{tail}"
            );
            data[stream]["truncated"] = Value::Bool(true);
            data[stream]["omitted_bytes"] = json!(omitted);
            Some(text)
        } else {
            (reduced.len() < text.len()).then_some(reduced)
        };
        if let Some(text) = text {
            data[stream]["text"] = json!(text);
            changed = true;
        }
    }
    if changed {
        result["output"] = source.clone();
    }
    changed.then_some(result)
}
