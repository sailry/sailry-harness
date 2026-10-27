//! Adapt shared Client decisions without reconstructing tool presentation in Dart.
use sailry_client::conversation::View;
use sailry_protocol::tool::Block;
use serde_json::{Value, json};

use sailry_client::conversation::details::{Limit, State, Tracker};

#[derive(Clone, serde::Deserialize)]
pub(super) struct Selection {
    entry: String,
    index: usize,
}
#[derive(serde::Deserialize)]
pub(super) struct Selected {
    pub items: Vec<Selection>,
    #[serde(default)]
    pub retry: Vec<Selection>,
}

pub(super) fn references(
    view: &View,
    selection: &[Selection],
) -> Vec<sailry_protocol::tool::detail::Reference> {
    let Some(snapshot) = &view.snapshot else {
        return Vec::new();
    };
    view.calls
        .iter()
        .filter(|call| {
            selection.iter().any(|selected| {
                selected.entry == call.source.entry && selected.index == call.source.index
            })
        })
        .filter_map(|call| call.detail_reference(&snapshot.page))
        .collect()
}

pub(super) fn view(view: &View, tracker: Option<&Tracker>) -> Result<String, String> {
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
            value["detail_state"] = Value::Null;
            value["detail"] = Value::Null;
            if let Some(tracker) = tracker
                && let Some(reference) = call.detail_reference(page)
            {
                value["detail_state"] = match tracker.state(&reference) {
                    State::Ready(detail) => {
                        value["detail"] = packet(&call.with_detail(detail), page)?;
                        json!({"kind":"ready"})
                    }
                    State::Loading => json!({"kind":"loading"}),
                    State::Failed { fault, retry_at } => {
                        json!({"kind":"failed","fault":fault,"retrying":retry_at.is_some()})
                    }
                    State::TooLarge(limit) => {
                        json!({"kind":"too_large","limit":match limit {Limit::Transfer=>"transfer",Limit::Memory=>"memory"}})
                    }
                    State::Superseded => Value::Null,
                };
            }
            value["children"] = json!(call.children(page));
            value["row"] = json!(call.row(page));
            value["approval_view"] = json!(call.approval_view(page));
            value["revision"] = json!(
                call.detail_reference(page)
                    .map(|reference| reference.revision)
            );
            value["published"] = json!(call.summary(page).is_some_and(|summary| summary.published));
            value["hidden"] = json!(call.hidden(page));
            value["details"] = json!(call.details(page));
            value["output"] = if call.details(page) {
                Value::Null
            } else {
                json!(call.output(page))
            };
        }
    }
    value["detail_errors"] = json!(
        tracker
            .map(|tracker| tracker
                .failures()
                .into_iter()
                .map(|(_, fault)| fault)
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
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
    let detail = match client.detail(reference).await.map_err(super::error)? {
        sailry_client::conversation::details::Read::Ready(detail) => detail,
        sailry_client::conversation::details::Read::TooLarge(limit) => {
            return Err(limit.message().into());
        }
        sailry_client::conversation::details::Read::Superseded => return Ok("null".into()),
    };
    serde_json::to_string(&packet(&call.with_detail(detail), &snapshot.page)?).map_err(super::error)
}

fn packet(
    call: &sailry_client::conversation::tools::Call,
    page: &sailry_protocol::conversation::Page,
) -> Result<Value, String> {
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
    Ok(json!({
        "arguments":call.arguments(page), "result":call.result(page),
        "resolved":call.resolved(), "content":call.content(), "texts":texts,
        "output":call.output(page),
        "approval_view":call.approval_view(page),
    }))
}

#[cfg(test)]
mod tests;
