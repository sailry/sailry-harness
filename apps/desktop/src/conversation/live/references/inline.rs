//! The input owns source and undo. This draft catalog retains targets so restoring
//! a deleted token with Undo restores its reference, without a second edit history.
use super::*;
use crate::conversation::highlights;
use gpui_kit::base::input::{
    InlineToken, InlineTokenClickEvent, InlineTokenError, InputContent, TextareaLink,
};
use gpui_kit::component::input::{InputToken, Textarea};
use gpui_kit::prelude::FluentBuilder as _;

pub(in crate::conversation::live) fn marker(reference: &Reference) -> String {
    format!("@{}", reference.label.trim_start_matches('@'))
}

fn id(reference: &Reference) -> SharedString {
    let target = serde_json::to_vec(&reference.target).expect("reference target serializes");
    format!("reference:{}", blake3::hash(&target)).into()
}

pub(in crate::conversation::live) fn token(reference: &Reference) -> InlineToken {
    InlineToken::new(id(reference), marker(reference))
}

/// Rehydrate a captured source with its immutable reference metadata. Live typing
/// never calls this: a manually typed matching label is not an admitted reference.
pub(in crate::conversation::live) fn content(
    text: &str,
    references: &[Reference],
) -> Result<InputContent, InlineTokenError> {
    let mut content = InputContent::new(text.to_owned());
    for link in source_links(text, references, false) {
        if let Some(reference) = references
            .iter()
            .find(|reference| marker(reference) == link.id)
        {
            content = content.with_token(link.range, token(reference))?;
        }
    }
    Ok(content)
}

pub(in crate::conversation::live) fn active_content(
    content: &InputContent,
    references: &[Reference],
) -> Vec<Reference> {
    references
        .iter()
        .filter(|reference| {
            content
                .tokens()
                .iter()
                .any(|span| span.token().id() == &id(reference))
        })
        .cloned()
        .collect()
}

pub(in crate::conversation::live) fn textarea(
    input: &Entity<TextareaState>,
    references: &[Reference],
) -> Textarea {
    let icons: BTreeMap<_, _> = references
        .iter()
        .filter_map(|reference| draft_icon(reference).map(|icon| (id(reference), icon)))
        .collect();
    Textarea::new(input).token(move |context, _, cx| {
        InputToken::new(context)
            .border_0()
            .when(!context.is_selected(), |token| {
                token
                    .bg(cx.theme().primary.opacity(0.12))
                    .text_color(cx.theme().primary)
            })
            .when_some(icons.get(context.token().id()), |token, icon| {
                token.icon(Icon::default().path(icon.clone()))
            })
    })
}

pub(super) fn draft_icon(reference: &Reference) -> Option<SharedString> {
    match reference.target {
        Target::Plugin(_) | Target::Skill { .. } => None,
        _ => Some(icon(reference)),
    }
}

pub(in crate::conversation::live) fn links(
    text: &str,
    references: &[Reference],
) -> Vec<TextareaLink> {
    source_links(text, references, true)
}

fn source_links(text: &str, references: &[Reference], historical: bool) -> Vec<TextareaLink> {
    let excluded = if historical {
        highlights::excluded(text)
    } else {
        vec![]
    };
    let mut links = Vec::new();
    for reference in references {
        let label = marker(reference);
        for (start, _) in text.match_indices(&label) {
            let range = start..start + label.len();
            if text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
                || text[range.end..].chars().next().is_some_and(|c| {
                    !c.is_whitespace()
                        && !matches!(c, ',' | ';' | '!' | '?' | '，' | '。' | '；' | '！' | '？')
                })
                || excluded
                    .iter()
                    .any(|span| span.start < range.end && span.end > range.start)
            {
                continue;
            }
            links.push(TextareaLink {
                range,
                id: label.clone().into(),
                icon: None,
            });
        }
    }
    // A label may contain spaces; prefer a complete longer label over its prefix.
    links.sort_by_key(|link| (link.range.start, std::cmp::Reverse(link.range.end)));
    let mut end = 0;
    links.retain(|link| {
        if link.range.start < end {
            return false;
        }
        end = link.range.end;
        true
    });
    links
}

#[cfg(test)]
pub(in crate::conversation::live) fn active(
    text: &str,
    references: &[Reference],
) -> Vec<Reference> {
    let links = links(text, references);
    references
        .iter()
        .filter(|reference| {
            let marker = marker(reference);
            links.iter().any(|link| link.id == marker)
        })
        .cloned()
        .collect()
}

impl View {
    pub(super) fn remember_reference(&mut self, mut reference: Reference) -> InlineToken {
        if let Some(existing) = self
            .references
            .selected
            .iter()
            .find(|item| item.target == reference.target)
        {
            return token(existing);
        }
        let label = reference
            .label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        reference.label = label.clone();
        let mut suffix = 2;
        while self
            .references
            .selected
            .iter()
            .any(|item| marker(item) == marker(&reference))
        {
            reference.label = format!("{label} ({suffix})");
            suffix += 1;
        }
        let token = token(&reference);
        self.references.selected.push(reference);
        token
    }

    pub(in crate::conversation::live) fn active_references(&self, cx: &App) -> Vec<Reference> {
        active_content(&self.input.read(cx).content(), &self.references.selected)
    }

    pub(in crate::conversation::live) fn activate_reference(
        &mut self,
        event: &InlineTokenClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            return;
        }
        let input = self.input.clone();
        let references = self.references.selected.clone();
        self.activate_input_token(&input, &references, event, window, cx);
    }

    pub(in crate::conversation::live) fn activate_input_token(
        &mut self,
        input: &Entity<TextareaState>,
        references: &[Reference],
        event: &InlineTokenClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !input
            .read(cx)
            .tokens()
            .iter()
            .any(|span| span.range() == event.range() && span.token() == event.token())
        {
            return;
        }
        if let Some(reference) = references
            .iter()
            .find(|reference| id(reference) == *event.token().id())
        {
            self.open_reference(reference.clone(), None, window, cx);
        }
    }
}

/// Picker icons stay separate from the literal @ markers in message text.
pub(in crate::conversation::live) fn icon(reference: &Reference) -> SharedString {
    use gpui_kit::component::IconNamed;
    let name = match &reference.target {
        Target::Plugin(_) => return "reicon:ui/puzzle-piece".into(),
        Target::Skill { .. } => return "reicon:school/book".into(),
        _ => Item::Reference(reference.clone()).icon(),
    };
    name.path()
}
