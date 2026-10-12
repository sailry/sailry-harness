//! Visibility and ownership are presentation guards; Node repeats admission checks.
use super::*;
use sailry_protocol::plugin::{self, card};

impl View {
    #[cfg(test)]
    pub(crate) fn card_instances(&self) -> Vec<Entity<crate::plugins::cards::Card>> {
        self.cards.borrow().values().cloned().collect()
    }

    pub(crate) fn suspend_cards(&self, cx: &mut App) {
        for card in self.cards.borrow().values() {
            card.update(cx, |card, cx| {
                card.suspend();
                cx.notify();
            });
        }
    }

    pub(crate) fn card_reference(&self, card: &card::Summary) -> Option<plugin::Reference> {
        let snapshot = self.history.snapshot.as_ref()?;
        let latest = snapshot.page.runs.iter().max_by_key(|run| run.sequence)?;
        if self.readonly() || !self.connected()
            || self.binding.client.target() != card.node
            || self.session() != Some(card.receipt.session)
            || self.binding.worktree != Some(card.worktree)
            || latest.turn != card.receipt.turn || latest.origin.is_some()
            || !matches!(latest.status, Status::Running | Status::Completed | Status::Failed)
            || !self.history.calls.iter().any(|call| {
                call.turn == card.receipt.turn && call.content.as_ref().is_some_and(|content| {
                    matches!(content.blocks.get(usize::from(card.receipt.block)), Some(sailry_protocol::tool::Block::Card(value)) if value == card)
                })
            }) { return None }
        self.node
            .snapshot
            .as_ref()?
            .plugins
            .iter()
            .find(|package| {
                package.enabled
                    && package.name == card.package.name
                    && package.digest == card.package.digest
            })
            .map(plugin::Summary::reference)
    }

    pub(super) fn interactive_card(
        &self,
        summary: card::Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut cards = self.cards.borrow_mut();
        let card = cards.entry(summary.receipt).or_insert_with(|| {
            let source = cx.entity().downgrade();
            cx.new(|cx| crate::plugins::cards::Card::new(source, self.binding.clone(), summary, cx))
        });
        card.clone().into_any_element()
    }
}
