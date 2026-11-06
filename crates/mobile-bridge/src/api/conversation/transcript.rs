use sailry_client::conversation::{View, images, transcript, turns};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn project(view: &View, value: &mut Value) -> Result<(), String> {
    let Some(snapshot) = &view.snapshot else {
        return Ok(());
    };
    let page = &snapshot.page;
    value["owners"] = serde_json::to_value(
        page.runs
            .iter()
            .map(|run| (run.turn, turns::owner(page, run.turn)))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(super::error)?;
    let timeline = transcript::owners(view);
    value["segments"] = serde_json::to_value(
        timeline
            .iter()
            .map(|turn| (*turn, transcript::segments(view, *turn)))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(super::error)?;
    value["timeline"] = serde_json::to_value(timeline).map_err(super::error)?;
    value["images"] =
        serde_json::to_value(images::referenced(page, &snapshot.drafts)).map_err(super::error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
