use super::*;
pub(super) use crate::process::output::model::normalize;

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
        let Some(text) = result
            .pointer(&format!(
                "{}{stream}/text",
                if wrapped { "/data/" } else { "/" }
            ))
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
            let data = if wrapped {
                result.get_mut("data")?
            } else {
                &mut result
            };
            data[stream]["text"] = json!(reduced);
            changed = true;
        }
    }
    if let Some(mut bounded) = super::bounded(&result, source) {
        let id = original
            .pointer("/sailry_command/id")
            .or_else(|| original.pointer(if wrapped { "/data/info/id" } else { "/info/id" }));
        let continuation = match id.and_then(Value::as_str) {
            Some(id) => {
                bounded["sailry_command"] = json!({"id":id});
                format!(
                    "read_command id={id} with stream, offset and limit for retained command output"
                )
            }
            None => {
                "read_command with output, stream, offset and limit for retained command output"
                    .into()
            }
        };
        if let Some(text) = bounded
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            bounded["text"] = json!(text.replace(
                "read_tool_output with output, offset and limit for the original result",
                &continuation,
            ));
        }
        return Some(bounded);
    }
    if changed {
        result["output"] = source.clone();
    }
    changed.then_some(result)
}
