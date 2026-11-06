use sailry_client::conversation::{View, guidance, turns};
use sailry_protocol::TurnId;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
struct Input {
    turn: TurnId,
    #[serde(flatten)]
    content: turns::InputView,
}

pub(super) fn project(view: &View, value: &mut Value) -> Result<(), String> {
    let Some(snapshot) = &view.snapshot else {
        return Ok(());
    };
    let page = &snapshot.page;
    let pending = |turn: &TurnId| view.guidance.get(turn) == Some(&guidance::State::Pending);
    let visible = turns::visible_turns(page, &snapshot.drafts);
    value["owners"] = serde_json::to_value(
        page.runs
            .iter()
            .map(|run| (run.turn, turns::owner(page, run.turn)))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(super::error)?;
    value["timeline"] = serde_json::to_value(
        view.timeline
            .iter()
            .copied()
            .filter(|turn| visible.contains(turn) && !pending(turn))
            .collect::<Vec<_>>(),
    )
    .map_err(super::error)?;
    value["inputs"] = serde_json::to_value(
        page.runs
            .iter()
            .map(|run| {
                let inputs: Vec<_> = turns::group(page, run.turn)
                    .into_iter()
                    .filter(|run| run.continuation.is_none() && !run.notice && !pending(&run.turn))
                    .flat_map(|run| {
                        turns::inputs(page, run.turn)
                            .into_iter()
                            .map(move |content| Input {
                                turn: run.turn,
                                content,
                            })
                    })
                    .collect();
                (run.turn, inputs)
            })
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(super::error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
