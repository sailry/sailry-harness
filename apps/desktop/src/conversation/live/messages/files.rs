use super::*;
use sailry_protocol::tool::Block;

impl View {
    pub(super) fn published_files(&self, turn: TurnId, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(snapshot) = &self.history.snapshot else {
            return Vec::new();
        };
        let mut paths = std::collections::HashSet::new();
        let mut documents = std::collections::HashSet::new();
        let mut groups = Vec::new();
        for call in self
            .history
            .calls
            .iter()
            .rev()
            .filter(|call| call.turn == turn)
        {
            if !call
                .summary(&snapshot.page)
                .is_some_and(|summary| summary.published)
            {
                continue;
            }
            let Some(reference) = call.detail_reference(&snapshot.page) else {
                continue;
            };
            let call = self.detailed(call, &snapshot.page);
            let mut cards = Vec::new();
            for (index, block) in call
                .content()
                .into_iter()
                .flat_map(|content| &content.blocks)
                .enumerate()
            {
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
            cards.reverse();
            let ready = call.detail.is_some();
            if ready && cards.is_empty() {
                self.publication_heights
                    .borrow_mut()
                    .insert(reference, px(0.));
                continue;
            }
            let content = if ready {
                v_flex()
                    .w_full()
                    .gap(cx.theme().spacing_tokens().lg)
                    .children(cards)
                    .into_any_element()
            } else {
                let height = self
                    .publication_heights
                    .borrow()
                    .get(&reference)
                    .copied()
                    .unwrap_or(DETAIL_LOADING_HEIGHT);
                if height == px(0.) {
                    continue;
                }
                let retry = reference.clone();
                let failed = matches!(
                    self.details.state(&reference),
                    sailry_client::conversation::details::State::Failed { retry_at: None, .. }
                        | sailry_client::conversation::details::State::TooLarge(_)
                );
                div()
                    .w_full()
                    .h(height)
                    .child(if failed {
                        crate::feedback::retry(format!("published-retry-{}", call.source.key()))
                            .on_click(cx.listener(move |view, _, _, _| view.details.retry(&retry)))
                            .into_any_element()
                    } else {
                        gpui_kit::component::spinner::Spinner::new().into_any_element()
                    })
                    .into_any_element()
            };
            let observe = self.detail_bounds([reference.clone()], true);
            let heights = self.publication_heights.clone();
            groups.push(
                div()
                    .w_full()
                    .child(content)
                    .on_prepaint(move |bounds, window, cx| {
                        if ready {
                            heights.borrow_mut().insert(reference, bounds.size.height);
                        }
                        observe(bounds, window, cx);
                    })
                    .into_any_element(),
            );
        }
        groups.reverse();
        groups
    }
}
