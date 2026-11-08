//! Browser element chips bind one composer token to its staged capture and
//! context drafts, which stay out of the attachment row. A send keeps only the
//! drafts of chips and attachment selections still present in the draft.
use super::*;
use crate::browser::annotation::{Details, Selection};
use crate::conversation::live::recovery;
use gpui_kit::component::input::{
    InlineToken, InlineTokenClickEvent, InlineTokenContext, InputContent, InputToken,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::conversation::reference::{Reference, Target};
use std::rc::Rc;

const PREFIX: &str = "element:";

type Bindings = Vec<(usize, usize)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::conversation::live) struct Chip {
    pub key: usize,
    pub label: String,
    pub details: Details,
}

impl Chip {
    pub(in crate::conversation::live) fn token(&self) -> InlineToken {
        InlineToken::new(format!("{PREFIX}{}", self.key), format!("@{}", self.label))
    }
}

/// One draft as seen by chip binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::conversation::live) struct Bound {
    pub key: usize,
    pub chip: Option<usize>,
    pub attachment: Option<AttachmentId>,
}

/// Chips whose token remains in the composer content.
pub(in crate::conversation::live) fn present(content: &InputContent) -> Vec<usize> {
    content
        .tokens()
        .iter()
        .filter_map(|span| span.token().id().strip_prefix(PREFIX)?.parse().ok())
        .collect()
}

/// Drafts that leave at send: those of removed chips and removed selections.
pub(in crate::conversation::live) fn removed(
    drafts: &[Bound],
    present: &[usize],
    selected: &[Reference],
    active: &[Reference],
) -> Vec<usize> {
    drafts
        .iter()
        .filter(|draft| {
            draft.chip.is_some_and(|chip| !present.contains(&chip))
                || draft.attachment.is_some_and(|id| {
                    sailry_client::conversation::references::retained(&[id], selected, active)
                        .is_empty()
                })
        })
        .map(|draft| draft.key)
        .collect()
}

/// One reference per published draft, labelled by its chip so they form one selection.
pub(in crate::conversation::live) fn references(
    chips: &[Chip],
    drafts: &[Bound],
    included: &[AttachmentId],
) -> Vec<Reference> {
    let mut references = Vec::new();
    for chip in chips {
        for draft in drafts.iter().filter(|draft| draft.chip == Some(chip.key)) {
            if let Some(id) = draft.attachment.filter(|id| included.contains(id)) {
                references.push(Reference {
                    target: Target::Attachment(id),
                    label: chip.label.clone(),
                });
            }
        }
    }
    references
}

/// Chips with the positions of their drafts among all drafts.
pub(in crate::conversation::live) fn positions(
    chips: &[Chip],
    drafts: &[Bound],
) -> Vec<recovery::Chip> {
    chips
        .iter()
        .map(|chip| recovery::Chip {
            key: chip.key,
            label: chip.label.clone(),
            details: chip.details.clone(),
            drafts: drafts
                .iter()
                .enumerate()
                .filter(|(_, draft)| draft.chip == Some(chip.key))
                .map(|(position, _)| position)
                .collect(),
        })
        .collect()
}

/// Recovered chips and `(position, chip)` bindings, or nothing for an inconsistent record.
pub(in crate::conversation::live) fn restore(
    chips: Vec<recovery::Chip>,
    count: usize,
) -> Option<(Vec<Chip>, Bindings)> {
    let mut restored: Vec<Chip> = Vec::new();
    let mut bindings: Bindings = Vec::new();
    for chip in chips {
        if chip.label.trim().is_empty()
            || restored
                .iter()
                .any(|other| other.key == chip.key || other.label == chip.label)
        {
            return None;
        }
        for position in chip.drafts {
            if position >= count || bindings.iter().any(|(other, _)| *other == position) {
                return None;
            }
            bindings.push((position, chip.key));
        }
        restored.push(Chip {
            key: chip.key,
            label: chip.label,
            details: chip.details,
        });
    }
    Some((restored, bindings))
}

impl Drafts {
    pub(in crate::conversation::live) fn bound(&self) -> Vec<Bound> {
        self.items
            .iter()
            .map(|item| Bound {
                key: item.key,
                chip: item.chip,
                attachment: match &item.status {
                    Status::Ready(attachment) => Some(attachment.id),
                    _ => None,
                },
            })
            .collect()
    }

    pub(super) fn retain_chips(&mut self) {
        let items = &self.items;
        self.chips
            .retain(|chip| items.iter().any(|item| item.chip == Some(chip.key)));
    }

    pub(in crate::conversation::live) fn chips(&self) -> &[Chip] {
        &self.chips
    }

    /// Restores chips captured with a failed send; their drafts keep their binding.
    pub(in crate::conversation::live) fn restore_chips(&mut self, chips: Vec<Chip>) {
        for chip in chips {
            if !self.chips.iter().any(|other| other.key == chip.key) {
                self.next_chip = self.next_chip.max(chip.key + 1);
                self.chips.push(chip);
            }
        }
    }

    /// Carries chips to a retargeted draft whose sources keep their bindings.
    pub(in crate::conversation::live) fn adopt_chips(&mut self, previous: &Self) {
        self.chips = previous.chips.clone();
        self.next_chip = previous.next_chip;
    }
}

#[derive(Clone)]
struct Card {
    id: SharedString,
    image: Option<images::ImageSource>,
    details: Option<Details>,
}

impl Card {
    fn render(&self, images: &Entity<images::Images>, cx: &App) -> AnyElement {
        v_flex()
            .gap_1()
            .max_w(px(320.))
            .min_w_0()
            .text_sm()
            .when_some(self.image.clone(), |card, source| {
                card.child(images::card(
                    &source,
                    images,
                    vec![source.clone()],
                    0,
                    images::Presentation::default(),
                    cx,
                ))
            })
            .when_some(self.details.clone(), |card, details| {
                card.when(!details.component.is_empty(), |card| {
                    card.child(div().min_w_0().truncate().child(details.component.clone()))
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(details.selector.clone()),
                )
                .when(!details.page.is_empty(), |card| {
                    card.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(details.page.clone()),
                    )
                })
            })
            .into_any_element()
    }
}

impl View {
    pub(crate) fn insert_browser_element(
        &mut self,
        selection: &Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        if self.attachments_blocked()
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return Err("browser_annotation_stale");
        }
        if self.attachments.items.len() + 2 > 8 {
            return Err("chat_attachment_limit");
        }
        let key = self.attachments.next_chip;
        let chip = Chip {
            key,
            label: self.unique_label(&selection.label),
            details: selection.details.clone(),
        };
        let name = format!("browser-element-{}", self.attachments.next);
        self.attach_drafts(
            vec![
                (
                    Source::Image {
                        name: format!("{name}.jpg"),
                        format: ImageFormat::Jpeg,
                        bytes: selection.image.clone(),
                    },
                    Some(key),
                ),
                (
                    Source::Data {
                        name: format!("{name}.json"),
                        media_type: "application/json",
                        bytes: selection.context.clone(),
                    },
                    Some(key),
                ),
            ],
            window,
            cx,
        );
        let drafts: Vec<_> = self
            .attachments
            .items
            .iter()
            .filter(|item| item.chip == Some(key))
            .map(|item| item.key)
            .collect();
        if drafts.len() != 2 {
            self.release_drafts(&drafts);
            return Err("browser_annotation_unavailable");
        }
        let token = chip.token();
        let note = selection.note.trim().to_owned();
        let inserted = self.input.update(cx, |input, cx| {
            let value = input.value();
            let start = input.selected_range().start;
            if value[..start]
                .chars()
                .next_back()
                .is_some_and(|letter| !letter.is_whitespace())
            {
                input.replace(" ", window, cx);
            }
            let result = input.replace_with_token(token, window, cx);
            if result.is_ok() {
                input.replace(
                    if note.is_empty() {
                        " ".to_owned()
                    } else {
                        format!(" {note} ")
                    },
                    window,
                    cx,
                );
            }
            input.focus(window, cx);
            result
        });
        if inserted.is_err() {
            self.release_drafts(&drafts);
            return Err("browser_annotation_stale");
        }
        self.attachments.next_chip += 1;
        self.attachments.chips.push(chip);
        self.attachments.error = None;
        self.references.dismiss();
        cx.notify();
        Ok(())
    }

    fn unique_label(&self, label: &str) -> String {
        let taken = |candidate: &str| {
            let marker = format!("@{candidate}");
            self.references
                .selected
                .iter()
                .any(|reference| super::super::references::inline::marker(reference) == marker)
                || self
                    .attachments
                    .chips
                    .iter()
                    .any(|chip| chip.label == candidate)
        };
        let mut candidate = label.to_owned();
        let mut suffix = 2;
        while taken(&candidate) {
            candidate = format!("{label} ({suffix})");
            suffix += 1;
        }
        candidate
    }

    /// Removes drafts without restoring them; published local uploads are discarded.
    fn release_drafts(&mut self, keys: &[usize]) {
        for item in self
            .attachments
            .items
            .iter()
            .filter(|item| keys.contains(&item.key))
        {
            if item.source.is_some()
                && let Status::Ready(attachment) = &item.status
            {
                let client = self.binding.client.clone();
                let pending = transfer::discard(&client, attachment);
                self.binding.runtime.spawn(async move {
                    if !matches!(transfer::check(client, pending).await, Status::Removed) {
                        eprintln!("Draft attachment cleanup did not complete");
                    }
                });
            }
        }
        self.attachments
            .items
            .retain(|item| !keys.contains(&item.key));
        self.attachments.retain_chips();
    }

    fn leaving(&self, active: &[Reference], cx: &App) -> Vec<usize> {
        removed(
            &self.attachments.bound(),
            &present(&self.input.read(cx).content()),
            &self.references.selected,
            active,
        )
    }

    /// Drafts that stay with the message can be sent.
    pub(in crate::conversation::live) fn drafts_sendable(&self, cx: &App) -> bool {
        self.attachments
            .sendable_except(&self.leaving(&self.active_references(cx), cx))
    }

    /// Drops drafts of removed chips and selections before a send captures the draft.
    pub(in crate::conversation::live) fn prune_drafts(&mut self, active: &[Reference], cx: &App) {
        let keys = self.leaving(active, cx);
        self.release_drafts(&keys);
    }

    pub(in crate::conversation::live) fn chip_references(
        &self,
        included: &[AttachmentId],
    ) -> Vec<Reference> {
        references(&self.attachments.chips, &self.attachments.bound(), included)
    }

    /// Bound drafts leave the row unless failed, so they can be retried or removed.
    pub(super) fn bound_draft(&self, item: &Item) -> bool {
        if matches!(
            item.status,
            Status::Failed(_) | Status::Uncertain(_) | Status::Rejected(_)
        ) {
            return false;
        }
        item.chip.is_some()
            || matches!(&item.status, Status::Ready(attachment) if self
                .references
                .selected
                .iter()
                .any(|reference| reference.target == Target::Attachment(attachment.id)))
    }

    /// Binds recovered chips to drafts restored in their original order.
    pub(in crate::conversation::live) fn recover_chips(
        &mut self,
        chips: Vec<Chip>,
        bindings: Vec<(usize, usize)>,
    ) {
        for (position, key) in bindings {
            if let Some(item) = self.attachments.items.get_mut(position) {
                item.chip = Some(key);
            }
        }
        self.attachments.restore_chips(chips);
        self.attachments.retain_chips();
    }

    fn chip_cards(&self) -> Vec<Card> {
        let mut cards: Vec<Card> = self
            .attachments
            .chips
            .iter()
            .map(|chip| Card {
                id: chip.token().id().clone(),
                image: self
                    .attachments
                    .items
                    .iter()
                    .filter(|item| item.chip == Some(chip.key))
                    .map(render::draft_source)
                    .find(|source| source.kind().is_some()),
                details: Some(chip.details.clone()),
            })
            .collect();
        let published = self.attachments.published();
        for reference in &self.references.selected {
            if !matches!(reference.target, Target::Attachment(_)) {
                continue;
            }
            let id = super::super::references::inline::token(reference)
                .id()
                .clone();
            if cards.iter().any(|card| card.id == id) {
                continue;
            }
            let image = sailry_client::conversation::references::selection(
                &self.references.selected,
                &reference.label,
            )
            .iter()
            .filter_map(|id| published.iter().find(|attachment| attachment.id == *id))
            .map(images::ImageSource::from)
            .find(|source| source.kind().is_some());
            cards.push(Card {
                id,
                image,
                details: None,
            });
        }
        cards
    }

    /// Element chips and selections show their capture and page details on hover.
    pub(in crate::conversation::live) fn chip_renderer(
        &self,
    ) -> impl Fn(&InlineTokenContext, &mut Window, &mut App) -> AnyElement + 'static {
        let cards = Rc::new(self.chip_cards());
        let images = self.images.clone();
        move |context: &InlineTokenContext, _: &mut Window, _: &mut App| {
            let token = InputToken::new(context);
            let Some(card) = cards
                .iter()
                .find(|card| &card.id == context.token().id())
                .cloned()
            else {
                return token.into_any_element();
            };
            let images = images.clone();
            hover_card::HoverCard::new(SharedString::from(format!(
                "composer-chip-{}-{}",
                card.id,
                context.range().start
            )))
            .trigger(token.icon(Icon::default().path("reicon:map/pointer")))
            .content(move |_, _, cx| card.render(&images, cx))
            .into_any_element()
        }
    }

    /// Opens a chip's capture; returns whether the token was an element chip.
    pub(in crate::conversation::live) fn activate_chip(
        &mut self,
        event: &InlineTokenClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(key) = event
            .token()
            .id()
            .strip_prefix(PREFIX)
            .and_then(|key| key.parse::<usize>().ok())
        else {
            return false;
        };
        let image = self
            .attachments
            .items
            .iter()
            .filter(|item| item.chip == Some(key))
            .map(render::draft_source)
            .find(|source| source.kind().is_some());
        if let Some(image) = image {
            images::Images::open_on_close(&self.images, image, |_, _| {}, window, cx);
        }
        true
    }
}

#[cfg(test)]
mod tests;
