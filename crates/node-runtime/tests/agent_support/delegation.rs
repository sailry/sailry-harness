//! Scripted provider behavior is an integration fixture, not model acceptance.
use super::*;

pub(super) fn next(body: &Value, role: Option<&str>) -> Option<(String, Value)> {
    let messages = body["messages"].as_array().unwrap();
    let latest = messages
        .iter()
        .rposition(|message| message["role"] == "user" && message["content"].is_string())
        .unwrap();
    let results: Vec<_> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            (message["role"] == "tool")
                .then(|| {
                    serde_json::from_str::<Value>(message["content"].as_str().unwrap())
                        .ok()
                        .map(|result| (index, result))
                })
                .flatten()
        })
        .collect();
    let child = results
        .iter()
        .find_map(|(_, result)| result["session"].as_str());
    let Some(child) = child else {
        return Some((
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":role,"task":"Perform the delegated writes","background":true}),
        ));
    };
    let correction = messages[latest]["content"]
        .as_str()
        .unwrap()
        .contains("Main correction");
    if correction
        && !results
            .iter()
            .any(|(index, result)| *index > latest && result["recipient"].is_string())
    {
        return Some((
            plugin_tool("delegation", "guide_agent"),
            json!({"session":child,"message":"Apply the child correction after all writes settle"}),
        ));
    }
    if results.iter().any(|(index, result)| {
        *index > latest
            && (result["paused"] == true
                || result["status"] == "completed" && result.get("response").is_some())
    }) {
        return None;
    }
    Some((
        plugin_tool("delegation", "wait_agent"),
        json!({"session":child}),
    ))
}
