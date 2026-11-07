//! Chat schema adaptation derived from Goose 14741e9a formats/openai.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.

use serde_json::{Value, json};

pub(super) fn chat(body: &mut Value) {
    for tool in body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(function) = tool.get_mut("function").and_then(Value::as_object_mut) else {
            continue;
        };
        function.insert("strict".into(), Value::Bool(false));
        if let Some(parameters) = function.get_mut("parameters")
            && let Some(object) = parameters.as_object_mut()
        {
            object.entry("type").or_insert(json!("object"));
            sanitize(parameters);
        }
    }
}

fn sanitize(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.contains_key("anyOf")
        && let Some(union) = object.remove("oneOf")
    {
        object.insert("anyOf".into(), union);
    }
    nullable(object);
    if object.get("type").and_then(Value::as_str) == Some("object") {
        object.entry("properties").or_insert(json!({}));
        object.entry("required").or_insert(json!([]));
    }
    for key in ["properties", "$defs", "definitions"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                sanitize(child);
            }
        }
    }
    for key in ["anyOf", "allOf", "prefixItems"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_array_mut) {
            for child in children {
                sanitize(child);
            }
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(child) = object.get_mut(key) {
            sanitize(child);
        }
    }
}

fn nullable(object: &mut serde_json::Map<String, Value>) {
    if let Some(types) = object.get("type").and_then(Value::as_array) {
        let mut values = types.iter().filter(|value| value.as_str() != Some("null"));
        if let Some(value) = values.next()
            && values.next().is_none()
        {
            object.insert("type".into(), value.clone());
            return;
        }
    }
    let Some(union) = object.remove("anyOf") else {
        return;
    };
    if let Some(variants) = union.as_array().filter(|values| values.len() == 2) {
        let non_null = if variants[0].get("type").and_then(Value::as_str) == Some("null") {
            Some(&variants[1])
        } else if variants[1].get("type").and_then(Value::as_str) == Some("null") {
            Some(&variants[0])
        } else {
            None
        };
        if let Some(replacement) = non_null.and_then(Value::as_object) {
            for (key, value) in replacement {
                object.entry(key.clone()).or_insert_with(|| value.clone());
            }
            return;
        }
    }
    object.insert("anyOf".into(), union);
}
