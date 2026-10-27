use super::*;

pub(super) fn project(original: &Value, source: &Value) -> Option<Value> {
    let mut result = original.clone();
    match original["kind"].as_str()? {
        "git_status" => sparse(&mut result["data"]),
        "git_log" => {
            for entry in result["data"]["entries"].as_array_mut()? {
                let message = entry["message"]
                    .as_str()?
                    .lines()
                    .filter(|line| {
                        !line.trim_start().starts_with("Signed-off-by:")
                            && !line.trim_start().starts_with("Co-authored-by:")
                    })
                    .filter(|line| !line.trim().is_empty())
                    .take(4)
                    .collect::<Vec<_>>()
                    .join("\n");
                entry["message"] = json!(message);
                let entry = entry.as_object_mut()?;
                entry.remove("email");
                entry.remove("timestamp");
            }
        }
        _ => return None,
    }
    let encoded = result.to_string();
    if encoded.len() > LIMIT {
        return Some(super::clip(&result, source, &encoded, 2 * EDGE, false));
    }
    result["output"] = source.clone();
    Some(result)
}

fn sparse(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|_, value| {
                !value.is_null()
                    && value != &Value::Bool(false)
                    && value != &Value::Number(0.into())
            });
            object.values_mut().for_each(sparse);
        }
        Value::Array(items) => items.iter_mut().for_each(sparse),
        _ => {}
    }
}
