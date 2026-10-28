//! Adapt shared Client decisions without reconstructing tool presentation in Dart.
use sailry_client::conversation::View;
use sailry_protocol::tool::Block;
use sailry_protocol::tool::detail::Reference;
use serde_json::{Value, json};
use std::collections::BTreeSet;

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

pub(super) fn select(view: &View, selection: &[Selection], tracker: &Tracker) {
    let references = references(view, selection);
    tracker.protect(view.calls.iter().filter_map(|call| {
        let page = &view.snapshot.as_ref()?.page;
        if !call.approval_pending(page) {
            return None;
        }
        let reference = call.detail_reference(page)?;
        references.contains(&reference).then_some(reference)
    }));
    tracker.set(references);
}

pub(super) fn view(
    view: &View,
    tracker: Option<&Tracker>,
    sent: &mut BTreeSet<Reference>,
) -> Result<String, String> {
    let mut ready = BTreeSet::new();
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
            let mut approval = call.approval_view(page);
            value["detail_state"] = Value::Null;
            value["detail"] = Value::Null;
            if let Some(tracker) = tracker
                && let Some(reference) = call.detail_reference(page)
            {
                value["detail_state"] = match tracker.state(&reference) {
                    State::Ready(detail) => {
                        ready.insert(reference.clone());
                        let detailed = call.with_detail(detail.detail.clone());
                        approval = detailed.approval_view(page);
                        if sent.insert(reference.clone()) {
                            value["detail"] = packet(&detailed, page)?;
                        } else {
                            value
                                .as_object_mut()
                                .expect("serialized call")
                                .remove("detail");
                        }
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
            value["approval_view"] = json!(approval);
            value["revision"] = json!(
                call.detail_reference(page)
                    .map(|reference| reference.revision)
            );
            value["detail_key"] = match call.detail_reference(page) {
                Some(reference) => {
                    Value::String(serde_json::to_string(&reference).map_err(super::error)?)
                }
                None => Value::Null,
            };
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
    sent.retain(|reference| ready.contains(reference));
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
    }))
}

#[cfg(test)]
mod tests;
