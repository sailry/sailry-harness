use super::*;
use sailry_protocol::tool::Block;

impl View {
    pub(super) fn published_files(&self, turn: TurnId, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(snapshot) = &self.history.snapshot else {
            return Vec::new();
        };
        let mut paths = std::collections::HashSet::new();
        let mut documents = std::collections::HashSet::new();
        let mut cards = Vec::new();
        for call in self
            .history
            .calls
            .iter()
            .rev()
            .filter(|call| call.turn == turn)
        {
            if call
                .summary(&snapshot.page)
                .is_some_and(|summary| summary.published)
            {
                self.request_details(call.detail_reference(&snapshot.page), false, cx);
            }
            let call = self.detailed(call, &snapshot.page);
            let Some(content) = call.content() else {
                continue;
            };
            for (index, block) in content.blocks.iter().enumerate() {
                if let Block::Card(card) = block {
                    cards.push(self.interactive_card(card.clone(), cx));
                    continue;
                }
                if let Block::Artifact { artifact } = block {
                    if documents.insert(artifact.id) {
                        cards.push(self.document_card(artifact.clone(), cx));
                    }
                    continue;
                }
                let Block::File(file) = block else {
                    continue;
                };
                if !paths.insert(file.path.clone()) {
                    continue;
                }
                let mut binding = self.binding.clone();
                binding.worktree = self.turn_worktree(turn);
                cards.push(
                    crate::content::files::Card {
                        id: format!("published-{}-{}-{index}", turn, call.source.key()).into(),
                        file: file.clone(),
                        binding,
                        source: cx.entity(),
                    }
                    .into_any_element(),
                );
            }
        }
        cards.reverse();
        cards
    }
}
