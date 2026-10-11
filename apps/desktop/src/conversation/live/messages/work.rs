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

impl View {
    pub(super) fn work_open(&self, turn: TurnId, active: bool) -> bool {
        self.expanded
            .get(&(turn, "work".to_owned()))
            .copied()
            .unwrap_or(active)
    }
}

#[cfg(test)]
mod tests;
