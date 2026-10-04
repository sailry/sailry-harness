//! Preview references composed with Kit List/Popover. The pinned ordinary Textarea has no
//! completion provider; only its input actions are bridged while this catalog is open.

mod catalog;
#[cfg(test)]
mod tests;
mod view;

use crate::tr;
use crate::{preview, shell::Shell, workspace::Owner};
pub(in crate::conversation) use catalog::Trigger;
use catalog::{Item, Key, Page};
pub(crate) use catalog::{Kind, Reference};
use gpui_kit::base::input::{InlineToken, InlineTokenClickEvent, InputContent};
use gpui_kit::component::input::{InputToken, Textarea, TextareaState};
use gpui_kit::component::{Icon, IndexPath, list::ListState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use view::Items;

fn id(reference: &Reference) -> SharedString {
    let target = format!("{:?}:{:?}", reference.owner, reference.kind);
    format!("preview-reference:{}", blake3::hash(target.as_bytes())).into()
}

fn token(reference: &Reference) -> InlineToken {
    InlineToken::new(
        id(reference),
        format!("@{}", reference.label.trim_start_matches('@')),
    )
}

pub(in crate::conversation) fn active(
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

pub(in crate::conversation) fn content(
    text: SharedString,
    references: &[Reference],
) -> InputContent {
    let mut content = InputContent::new(text.clone());
    let mut ranges = references
        .iter()
        .flat_map(|reference| {
            let token = token(reference);
            text.match_indices(token.text().as_str())
                .map(|(start, _)| (start..start + token.text().len(), token.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.end)));
    let mut end = 0;
    for (range, token) in ranges {
        if range.start < end {
            continue;
        }
        end = range.end;
        content = content
            .with_token(range, token)
            .expect("preview reference source matches");
    }
    content
}

pub(in crate::conversation) fn textarea(
    input: &Entity<TextareaState>,
    references: &[Reference],
) -> Textarea {
    let references = references.to_vec();
    Textarea::new(input).token(move |context, _, _| {
        InputToken::new(context).when_some(
            references
                .iter()
                .find(|reference| context.token().id() == &id(reference)),
            |token, reference| token.icon(Icon::new(reference.icon())),
        )
    })
}

pub(crate) struct State {
    pub(super) open: bool,
    trigger: Option<Trigger>,
    dismissed: Option<Trigger>,
    path: Vec<Page>,
    owner: Owner,
    generation: u64,
    list: Entity<ListState<Items>>,
    pub(super) error: Option<&'static str>,
}

impl State {
    pub fn new(
        shell: WeakEntity<Shell>,
        key: Key,
        owner: Owner,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self {
            open: false,
            trigger: None,
            dismissed: None,
            path: Vec::new(),
            owner,
            generation: 0,
            list: cx.new(|cx| ListState::new(Items::new(shell, key), window, cx).searchable(false)),
            error: None,
        }
    }

    pub fn dismiss(&mut self) {
        self.generation += 1;
        self.open = false;
        self.dismissed = self.trigger.clone();
        self.path.clear();
    }
}

impl Shell {
    pub(in crate::conversation) fn activate_reference(
        &mut self,
        key: Key,
        event: &InlineTokenClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if key != (self.host, self.session) || self.page != preview::Page::Conversation {
            return;
        }
        let Some(thread) = self.conversations.get(&key) else {
            return;
        };
        if !thread
            .input
            .read(cx)
            .tokens()
            .iter()
            .any(|span| span.range() == event.range() && span.token() == event.token())
        {
            return;
        }
        let Some(reference) = thread
            .options
            .references
            .iter()
            .find(|reference| id(reference) == *event.token().id())
            .cloned()
        else {
            return;
        };
        if self
            .workspace
            .sessions
            .get(&key)
            .is_none_or(|session| session.owner != reference.owner)
        {
            return;
        }
        match reference.kind {
            Kind::File(path) => self.open_conversation_link(path.into(), window, cx),
            Kind::Directory(_) => self.open_resource_panel(preview::Page::Files, window, cx),
            _ => {}
        }
    }

    pub(crate) fn refresh_references(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != preview::Page::Conversation || key != (self.host, self.session) {
            return;
        }
        let Some(thread) = self.conversations.get(&key) else {
            return;
        };
        let input = thread.input.clone();
        let trigger = input.update(cx, |input, cx| {
            if input.marked_text_range(window, cx).is_some() {
                return None;
            }
            Trigger::parse(input.value(), input.selected_range()).filter(|trigger| {
                !input.tokens().iter().any(|span| {
                    let range = span.range();
                    range.start < trigger.range.end && trigger.range.start < range.end
                })
            })
        });
        let owner = self.workspace.sessions[&key].owner;
        let state = &mut self.conversations.get_mut(&key).unwrap().references;
        if state.trigger == trigger && state.owner == owner {
            return;
        }
        if trigger != state.dismissed {
            state.dismissed = None;
        }
        if state.owner != owner
            || !state.open
            || state.trigger.as_ref().map(|token| token.range.start)
                != trigger.as_ref().map(|token| token.range.start)
        {
            state.path.clear();
        }
        state.owner = owner;
        state.trigger = trigger;
        state.open = state.trigger.is_some() && state.trigger != state.dismissed;
        if !state.open {
            cx.notify();
            return;
        }
        self.reference_rows(key, window, cx);
        cx.notify();
    }

    fn reference_rows(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut self.conversations.get_mut(&key).unwrap().references;
        state.generation += 1;
        let rows = catalog::items(
            state.path.last().unwrap_or(&Page::Root),
            state.owner,
            &self.workspace,
            self.settings.read(cx).role_profiles(),
            state
                .trigger
                .as_ref()
                .map_or("", |trigger| trigger.query.as_str()),
        );
        state.list.update(cx, |list, cx| {
            list.delegate_mut().rows = rows;
            list.delegate_mut().generation = state.generation;
            let selected = (!list.delegate().rows.is_empty()).then_some(IndexPath::default());
            list.set_selected_index(selected, window, cx);
            list.scroll_to_selected_item(window, cx);
            cx.notify();
        });
    }

    fn choose_reference(
        &mut self,
        key: Key,
        generation: u64,
        item: Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if key != (self.host, self.session) || self.page != preview::Page::Conversation {
            return;
        }
        let Some(thread) = self.conversations.get(&key) else {
            return;
        };
        let state = &thread.references;
        if state.generation != generation {
            return;
        }
        let input = thread.input.clone();
        if input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let Some(trigger) = state.trigger.clone().filter(|_| state.open) else {
            return;
        };
        if input.read(cx).value() != trigger.text
            || input.read(cx).selected_range() != (trigger.range.end..trigger.range.end)
            || self.workspace.sessions[&key].owner != state.owner
        {
            self.conversations
                .get_mut(&key)
                .unwrap()
                .references
                .dismiss();
            cx.notify();
            return;
        }
        let replacement = match item {
            Item::Page(page, ..) => {
                self.conversations
                    .get_mut(&key)
                    .unwrap()
                    .references
                    .path
                    .push(page);
                "@"
            }
            Item::Reference(mut reference, _) => {
                let error = if !reference.valid(
                    state.owner,
                    &self.workspace,
                    self.settings.read(cx).role_profiles(),
                ) {
                    Some("reference_stale")
                } else if let Kind::Agent(role) = &reference.kind
                    && let Err(error) = self.settings.read(cx).validate_role_options(role)
                {
                    Some(error)
                } else if matches!(reference.kind, Kind::Agent(_))
                    && active(&thread.input.read(cx).content(), &thread.options.references)
                        .iter()
                        .any(|selected| {
                            matches!(selected.kind, Kind::Agent(_)) && selected != &reference
                        })
                {
                    Some("reference_one_agent")
                } else {
                    None
                };
                let thread = self.conversations.get_mut(&key).unwrap();
                thread.references.error = error;
                if let Some(error) = error {
                    crate::feedback::error(&tr("reference_title"), &tr(error), window, cx);
                    thread.references.dismiss();
                    input.update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                    return;
                }
                if !thread
                    .options
                    .references
                    .iter()
                    .any(|selected| selected.same_target(&reference))
                {
                    let label = reference
                        .label
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    reference.label = label.clone().into();
                    let mut suffix = 2;
                    while thread
                        .options
                        .references
                        .iter()
                        .any(|selected| token(selected).text() == token(&reference).text())
                    {
                        reference.label = format!("{label} ({suffix})").into();
                        suffix += 1;
                    }
                    thread.options.references.push(reference.clone());
                }
                let reference = thread
                    .options
                    .references
                    .iter()
                    .find(|selected| selected.same_target(&reference))
                    .unwrap();
                let token = token(reference);
                thread.references.dismiss();
                let result = input.update(cx, |input, cx| {
                    let result = input.replace_range_with_token(trigger.range, token, window, cx);
                    if result.is_ok() {
                        input.replace(" ", window, cx);
                    }
                    input.focus(window, cx);
                    result
                });
                if result.is_err() {
                    crate::feedback::error(
                        &tr("reference_title"),
                        &tr("reference_stale"),
                        window,
                        cx,
                    );
                }
                self.refresh_references(key, window, cx);
                cx.notify();
                return;
            }
        };
        input.update(cx, |input, cx| {
            input.set_selected_range(trigger.range, cx);
            input.replace(replacement, window, cx);
            input.focus(window, cx);
        });
        // Programmatic Textarea edits do not emit Change; keep the directory in sync.
        self.refresh_references(key, window, cx);
        if replacement == "@" {
            self.reference_rows(key, window, cx);
        }
        cx.notify();
    }

    fn reference_back(
        &mut self,
        key: Key,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if key != (self.host, self.session) || self.page != preview::Page::Conversation {
            return;
        }
        let thread = self.conversations.get_mut(&key).unwrap();
        if thread.references.generation != generation || !thread.references.open {
            return;
        }
        if let Some(trigger) = thread.references.trigger.clone()
            && thread.input.read(cx).value() == trigger.text
            && self.workspace.sessions[&key].owner == thread.references.owner
        {
            thread.references.path.pop();
            thread.input.update(cx, |input, cx| {
                input.set_selected_range(trigger.range, cx);
                input.replace("@", window, cx);
                input.focus(window, cx);
            });
            self.refresh_references(key, window, cx);
            self.reference_rows(key, window, cx);
            cx.notify();
        }
    }

    pub(crate) fn reference_action(
        &mut self,
        key: Key,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let thread = &self.conversations[&key];
        if !thread.references.open {
            cx.propagate();
            return;
        }
        let input = thread.input.clone();
        if input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            cx.propagate();
            return;
        }
        let list = thread.references.list.clone();
        match action {
            "up" | "down" => list.update(cx, |list, cx| {
                let count = list.delegate().rows.len();
                if count > 0 {
                    let selected = list.selected_index().map_or(0, |index| index.row);
                    let next = (selected + if action == "up" { count - 1 } else { 1 }) % count;
                    list.set_selected_index(Some(IndexPath::new(0).row(next)), window, cx);
                    list.scroll_to_selected_item(window, cx);
                    cx.notify();
                }
            }),
            "enter" | "tab" => {
                let selected = list
                    .read(cx)
                    .selected_index()
                    .and_then(|index| list.read(cx).delegate().rows.get(index.row).cloned());
                if let Some(item) = selected {
                    let generation = list.read(cx).delegate().generation;
                    self.choose_reference(key, generation, item, window, cx);
                }
            }
            "escape" => self
                .conversations
                .get_mut(&key)
                .unwrap()
                .references
                .dismiss(),
            "backspace"
                if thread
                    .references
                    .trigger
                    .as_ref()
                    .is_some_and(|trigger| trigger.query.is_empty())
                    && !thread.references.path.is_empty() =>
            {
                self.reference_back(key, thread.references.generation, window, cx)
            }
            _ => {
                cx.propagate();
                return;
            }
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    pub(crate) fn validate_references(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(owner) = self
            .workspace
            .sessions
            .get(&key)
            .map(|session| session.owner)
        else {
            return false;
        };
        let thread = self.conversations.get_mut(&key).unwrap();
        let error = active(&thread.input.read(cx).content(), &thread.options.references)
            .iter()
            .find_map(|reference| {
                if !reference.valid(
                    owner,
                    &self.workspace,
                    self.settings.read(cx).role_profiles(),
                ) {
                    return Some("reference_stale");
                }
                if let Kind::Agent(role) = &reference.kind {
                    self.settings.read(cx).validate_role_options(role).err()
                } else {
                    None
                }
            });
        thread.references.error = error;
        if let Some(error) = error {
            crate::feedback::error(&tr("reference_title"), &tr(error), window, cx);
            cx.notify();
        }
        error.is_none()
    }
}
