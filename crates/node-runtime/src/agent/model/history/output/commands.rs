use super::*;
pub(super) use crate::process::output::model::normalize;

pub(super) fn project(
    original: &Value,
    source: &Value,
    arguments: Option<&Value>,
) -> Option<Value> {
    let mut changed = false;
    let mut result = original.clone();
    if let Some(retained) = original.get("sailry_command") {
        for stream in ["stdout", "stderr"] {
            if retained[stream].is_object() {
                result["data"][stream] = retained[stream].clone();
                changed = true;
            }
        }
        result.as_object_mut()?.remove("sailry_command");
        changed = true;
    }
    let command = arguments
        .and_then(|arguments| arguments["command"].as_str())
        .or_else(|| {
            original
                .pointer("/data/info/command")
                .and_then(Value::as_str)
        });
    for stream in ["stdout", "stderr"] {
        let Some(text) = result
            .pointer(&format!("/data/{stream}/text"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let normalized = normalize(text);
        let reduced = command
            .and_then(|command| crate::process::output::model::cargo(command, &normalized))
            .filter(|text| text.len() < normalized.len())
            .unwrap_or(normalized);
        if reduced.len() < text.len() {
            result["data"][stream]["text"] = json!(reduced);
            changed = true;
        }
    }
    if changed {
        result["output"] = source.clone();
    }
    changed.then_some(result)
}
