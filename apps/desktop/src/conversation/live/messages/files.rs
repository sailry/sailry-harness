use super::*;
use crate::content::images::{self, ImageSource};
use sailry_protocol::tool::{Block, File};

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
                        cards.push(self.document_card(artifact.clone(), turn, cx));
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
                if let Some(image) = image_source(file, binding.worktree) {
                    cards.push(super::super::attachments::source_links(
                        vec![image],
                        &binding,
                        &self.images,
                        MessageAlignment::Start,
                        cx,
                    ));
                    continue;
                }
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
            let heights = self.publication_heights.clone();
            groups.push(
                div()
                    .w_full()
                    .child(content)
                    .on_children_prepainted(move |bounds, _, _| {
                        if ready && let Some(bounds) = bounds.first() {
                            heights
                                .borrow_mut()
                                .insert(reference.clone(), bounds.size.height);
                        }
                    })
                    .into_any_element(),
            );
        }
        groups.reverse();
        groups
    }
}

fn image_source(file: &File, worktree: Option<WorktreeId>) -> Option<ImageSource> {
    let worktree = worktree?;
    images::displayable(&file.path).then(|| ImageSource::File {
        worktree,
        path: file.path.clone(),
        context: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn file(path: &str) -> File {
        File {
            path: path.into(),
            name: None,
            mime: crate::content::files::mime(path),
            size: None,
            revision: None,
        }
    }

    #[test]
    fn generated_assets() {
        for (scenario, check) in [
            (
                "generated_images_use_turn_worktree",
                (|_scenario: &str| {
                    let worktree = WorktreeId::new();
                    let output = file("assets/generated/result.png");
                    let source = image_source(&output, Some(worktree)).unwrap();
                    assert!(
                        matches!(
                            source,
                            ImageSource::File { worktree: captured, path, context: None }
                                if captured == worktree && path == output.path
                        ),
                        "generated images use turn worktree"
                    );
                }) as fn(&str),
            ),
            (
                "unsupported_outputs_keep_file_actions",
                (|_scenario: &str| {
                    let worktree = Some(WorktreeId::new());
                    for path in ["report.pdf", "document.html", "assets/generated/result.mp4"] {
                        assert!(
                            image_source(&file(path), worktree).is_none(),
                            "unsupported outputs keep file actions: {path}"
                        );
                    }
                    assert!(
                        image_source(&file("assets/generated/result.png"), None).is_none(),
                        "unsupported outputs keep file actions"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
