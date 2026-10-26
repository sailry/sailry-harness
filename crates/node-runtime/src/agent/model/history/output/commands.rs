use super::*;
pub(super) use crate::process::output::model::normalize;

pub(super) fn project(
    original: &Value,
    source: &Value,
    _arguments: Option<&Value>,
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
    for stream in ["stdout", "stderr"] {
        let Some(text) = result
            .pointer(&format!("/data/{stream}/text"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let reduced = normalize(text);
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
