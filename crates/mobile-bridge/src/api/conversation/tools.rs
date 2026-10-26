//! Adapt shared Client decisions without reconstructing tool presentation in Dart.
use sailry_client::conversation::View;
use sailry_protocol::tool::Block;
use serde_json::{Value, json};

pub(super) fn view(view: &View) -> Result<String, String> {
    let mut value = serde_json::to_value(view).map_err(super::error)?;
    if let Some(snapshot) = &view.snapshot {
        let page = &snapshot.page;
        value["rows"] = serde_json::to_value(
            page.runs
                .iter()
                .map(|run| {
                    (
                        run.turn,
                        sailry_client::conversation::rows::collect(snapshot, &view.calls, run.turn),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .map_err(super::error)?;
        value["inputs"] = serde_json::to_value(
            page.runs
                .iter()
                .map(|run| {
                    (
                        run.turn,
                        sailry_client::conversation::turns::inputs(page, run.turn),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .map_err(super::error)?;
        let calls = value["calls"]
            .as_array_mut()
            .expect("serialized calls are an array");
        for (value, call) in calls.iter_mut().zip(view.calls.iter()) {
            value["children"] = json!(call.children(page));
            value["row"] = json!(call.row(page));
            value["revision"] = json!(
                call.detail_reference(page)
                    .map(|reference| reference.revision)
            );
            value["published"] = json!(call.summary(page).is_some_and(|summary| summary.published));
            value["hidden"] = json!(call.hidden(page));
            value["details"] = json!(call.details(page));
            let deferred = call.details(page);
            value["output_deferred"] = json!(deferred);
            value["output"] = if deferred {
                Value::Null
            } else {
                json!(call.output(page))
            };
            value["texts"] = json!([]);
        }
    }
    serde_json::to_string(&value).map_err(super::error)
}

pub(super) async fn output(
    client: &sailry_client::Client,
    view: &View,
    entry: &str,
    index: usize,
) -> Result<String, String> {
    let snapshot = view
        .snapshot
        .as_ref()
        .ok_or("conversation snapshot is unavailable")?;
    let call = view
        .calls
        .iter()
        .find(|call| call.source.entry == entry && call.source.index == index)
        .ok_or("tool call is no longer available")?;
    let reference = call
        .detail_reference(&snapshot.page)
        .ok_or("tool details are unavailable")?;
    let detail = client.tool_detail(reference).await.map_err(super::error)?;
    packet(&call.with_detail(detail), &snapshot.page)
}

fn packet(
    call: &sailry_client::conversation::tools::Call,
    page: &sailry_protocol::conversation::Page,
) -> Result<String, String> {
    let result = call.result(page).unwrap_or(&Value::Null);
    let texts: Vec<_> = call
        .content()
        .into_iter()
        .flat_map(|content| &content.blocks)
        .map(|block| match block {
            Block::Text(text) => text.read(result),
            _ => None,
        })
        .collect();
    serde_json::to_string(&json!({
        "arguments":call.arguments(page), "result":call.result(page),
        "resolved":call.resolved(), "content":call.content(), "texts":texts,
        "output":call.output(page),
    }))
    .map_err(super::error)
}

#[cfg(test)]
mod tests;
