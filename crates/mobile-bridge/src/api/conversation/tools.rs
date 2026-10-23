//! Adapt shared Client decisions without reconstructing tool presentation in Dart.
use sailry_client::conversation::View;
use sailry_protocol::tool::Block;
use serde_json::{Value, json};

pub(super) fn view(view: &View) -> Result<String, String> {
    let mut value = serde_json::to_value(view).map_err(super::error)?;
    if let Some(snapshot) = &view.snapshot {
        let page = &snapshot.page;
        let calls = value["calls"]
            .as_array_mut()
            .expect("serialized calls are an array");
        for (value, call) in calls.iter_mut().zip(view.calls.iter()) {
            value["hidden"] = json!(call.hidden(page));
            value["details"] = json!(call.details(page));
            let deferred = call
                .resolved
                .as_ref()
                .and_then(|resolved| resolved.output.as_ref())
                .is_some_and(|output| output.preview_deferred);
            value["output_deferred"] = json!(deferred);
            value["output"] = if deferred {
                Value::Null
            } else {
                json!(call.output(page))
            };
            let result = call.result(page).unwrap_or(&Value::Null);
            value["texts"] = json!(
                call.content
                    .as_ref()
                    .into_iter()
                    .flat_map(|content| &content.blocks)
                    .map(|block| match block {
                        Block::Text(text) => text.read(result),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
        }
    }
    serde_json::to_string(&value).map_err(super::error)
}

pub(super) fn output(view: &View, entry: &str, index: usize) -> Result<String, String> {
    let snapshot = view
        .snapshot
        .as_ref()
        .ok_or("conversation snapshot is unavailable")?;
    let call = view
        .calls
        .iter()
        .find(|call| call.source.entry == entry && call.source.index == index)
        .ok_or("tool call is no longer available")?;
    serde_json::to_string(&call.output(&snapshot.page)).map_err(super::error)
}

#[cfg(test)]
mod tests;
