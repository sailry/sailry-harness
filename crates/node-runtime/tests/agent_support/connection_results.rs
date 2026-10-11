//! Scripted model calls use the exact connection identity returned by earlier real tools.
use serde_json::Value;

pub(super) fn resolve(body: &Value, call: &(String, Value)) -> (String, Value) {
    let profile = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| serde_json::from_str::<Value>(message["content"].as_str()?).ok())
        .find_map(profile);
    let mut arguments = call.1.clone();
    fn replace(value: &mut Value, profile: Option<&Value>) {
        match value {
            Value::String(text) if text == "$connection" || text == "$revision" => {
                let field = if text == "$connection" {
                    "id"
                } else {
                    "revision"
                };
                *value = profile.expect("earlier connection save must succeed")[field].clone();
            }
            Value::Object(fields) => {
                for value in fields.values_mut() {
                    replace(value, profile);
                }
            }
            Value::Array(values) => {
                for value in values {
                    replace(value, profile);
                }
            }
            _ => {}
        }
    }
    replace(&mut arguments, profile.as_ref());
    (call.0.clone(), arguments)
}

fn profile(value: Value) -> Option<Value> {
    if value["id"].is_string() && value["revision"].is_number() && value["name"].is_string() {
        return Some(value);
    }
    match value {
        Value::Object(fields) => fields.into_values().find_map(profile),
        Value::Array(values) => values.into_iter().find_map(profile),
        _ => None,
    }
}
