//! Chronological transcript segments shared by every controller.
//! Delivered input divides its response owner's reply at its canonical position.
use super::{
    View, children, guidance,
    rows::{self, Row},
    turns::{self, InputView},
};
use sailry_protocol::TurnId;

/// An input keeps its source turn inside its response owner's segment.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Input {
    pub turn: TurnId,
    #[serde(flatten)]
    pub content: InputView,
}

/// Inputs followed by the reply rows produced before the next delivered input.
#[derive(Default, serde::Serialize)]
pub struct Segment<'a> {
    pub inputs: Vec<Input>,
    pub rows: Vec<Row<'a>>,
    /// Foreground children holding the next model request; latest segment only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Position {
    /// The response owner's own input opens the turn.
    Start,
    /// Canonical sequence of a delivered joined input.
    At(u64),
    /// A claimed input whose canonical entry has not arrived.
    End,
}

/// Visible response owners in shared transcript order; pending guidance stays in the queue.
pub fn owners(view: &View) -> Vec<TurnId> {
    let Some(snapshot) = &view.snapshot else {
        return Vec::new();
    };
    let visible = turns::visible_turns(&snapshot.page, &snapshot.drafts);
    view.timeline
        .iter()
        .copied()
        .filter(|turn| visible.contains(turn) && !pending(view, *turn))
        .collect()
}

/// Splits one response owner's inputs and rows at each delivered input.
/// Inputs without rows between them share a segment; drafts follow committed rows.
pub fn segments(view: &View, turn: TurnId) -> Vec<Segment<'_>> {
    let Some(snapshot) = view.snapshot.as_deref() else {
        return Vec::new();
    };
    let page = &snapshot.page;
    let mut inputs = Vec::new();
    for run in turns::group(page, turn) {
        if run.continuation.is_some() || run.notice || pending(view, run.turn) {
            continue;
        }
        let mut sequences = page
            .entries
            .iter()
            .filter(|entry| entry.turn == run.turn && entry.author == "user")
            .map(|entry| entry.sequence);
        for content in turns::inputs(page, run.turn) {
            let position = if run.turn == turn {
                Position::Start
            } else if content.canonical {
                sequences.next().map_or(Position::End, Position::At)
            } else {
                Position::End
            };
            inputs.push((
                position,
                Input {
                    turn: run.turn,
                    content,
                },
            ));
        }
    }
    inputs.sort_by_key(|(position, _)| *position);
    let mut rows = rows::collect(snapshot, &view.calls, turn)
        .into_iter()
        .peekable();
    let mut segments = Vec::new();
    let mut current = Segment::default();
    let mut claimed = Vec::new();
    for (position, input) in inputs {
        match position {
            Position::Start => current.inputs.push(input),
            Position::End => claimed.push(input),
            Position::At(sequence) => {
                while let Some(row) =
                    rows.next_if(|row| row.entry.is_some_and(|entry| entry.sequence < sequence))
                {
                    current.rows.push(row);
                }
                if !current.rows.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                current.inputs.push(input);
            }
        }
    }
    current.rows.extend(rows);
    current.inputs.extend(claimed);
    current.waiting = children::waiting(view, turn, &current.rows);
    segments.push(current);
    segments
}

fn pending(view: &View, turn: TurnId) -> bool {
    view.guidance.get(&turn) == Some(&guidance::State::Pending)
}

#[cfg(test)]
mod tests;
