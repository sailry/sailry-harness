//! Bezel's transcript Work zone, composed with Kit disclosure controls.
//! Source: crabtalk/bezel 4a7505a, apps/gallery/src/patterns/transcript.rs (MIT).
//! Client events still determine order; expansion belongs to the existing view.
use super::*;

pub(super) fn answer_from(blocks: &[Block<'_>]) -> usize {
    blocks
        .iter()
        .rposition(|block| {
            !matches!(
                block,
                Block::Text(..) | Block::Search(..) | Block::Image(..)
            )
        })
        .map_or(0, |index| index + 1)
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
