//! Bezel's transcript Work zone, composed with Kit disclosure controls.
//! Source: crabtalk/bezel 4a7505a, apps/gallery/src/patterns/transcript.rs (MIT).
//! Client events still determine order; expansion belongs to the existing view.
use super::*;

pub(super) fn answer_from(blocks: &[Block<'_>]) -> usize {
    sailry_client::conversation::reply::boundary(blocks.iter().map(|block| match block {
        Block::Text(..) | Block::Search(..) | Block::Images(..) | Block::Question(_) => false,
        Block::Tools(calls) => {
            calls.is_empty() || calls.iter().any(|call| !call.terminal_interaction)
        }
        _ => true,
    }))
}

/// Expansion key of one segment's work; the first keeps the turn's original key.
pub(super) fn key(segment: usize) -> String {
    match segment {
        0 => "work".to_owned(),
        segment => format!("work-{segment}"),
    }
}

/// An earlier segment discloses its own work; only the last carries the turn status.
pub(super) fn header(
    turn: TurnId,
    segment: usize,
    open: bool,
    cx: &mut Context<View>,
) -> MessageHeader {
    MessageHeader::new().content_inset(false).w_full().child(
        v_flex()
            .text_sm()
            .w_full()
            .gap_2()
            .child(
                crate::conversation::surface::trigger(
                    format!("live-turn-work-{turn}-{segment}"),
                    tr("turn_work"),
                    tr("turn_work"),
                    open,
                    cx,
                )
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key(segment), !open, cx)),
                ),
            )
            .child(Separator::horizontal()),
    )
}

impl View {
    pub(super) fn work_open(&self, turn: TurnId, segment: usize, active: bool) -> bool {
        self.expanded
            .get(&(turn, key(segment)))
            .copied()
            .unwrap_or(active)
    }
}

#[cfg(test)]
mod tests;
