//! Deterministic model replies reconsider the latest complete memory tool result.
use serde_json::Value;

pub(super) fn resolve(body: &Value, call: &(String, Value)) -> (String, Value) {
    let latest = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| serde_json::from_str::<Value>(message["content"].as_str()?).ok())
        .find_map(|value| {
            let value = value.get("context").unwrap_or(&value);
            value
                .get("latest")?
                .as_array()?
                .iter()
                .flat_map(|entry| entry["parts"].as_array().unwrap())
                .filter_map(|part| serde_json::from_str::<Value>(part["text"].as_str()?).ok())
                .find(|entry| entry["body_truncated"] == false)
        });
    let mut arguments = call.1.clone();
    if arguments["expected_revision"] == "$memory_revision" {
        arguments["expected_revision"] = latest.as_ref().expect("latest complete memory expected")
            ["summary"]["revision"]
            .clone();
    }
    if arguments["body"] == "$memory_append" {
        arguments["body"] = format!(
            "{}; keep tests runnable",
            latest.as_ref().expect("latest complete memory expected")["body"]
                .as_str()
                .unwrap()
        )
        .into();
    }
    (call.0.clone(), arguments)
}
