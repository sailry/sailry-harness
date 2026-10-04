pub(super) mod presentation;
mod view;

use super::{Options, models::Selection, turn::Turn};
use crate::{shell::Shell, workspace::Owner};
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub session: (usize, usize),
    pub id: usize,
    pub revision: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: usize,
    pub revision: usize,
    pub text: SharedString,
    pub options: Options,
    pub owner: Owner,
}

impl Entry {
    fn label(&self) -> SharedString {
        if self.text.trim().is_empty() {
            self.options
                .references
                .first()
                .map(|reference| reference.label.clone())
                .unwrap_or_else(|| crate::tr("composer_example_file"))
        } else {
            self.text.clone()
        }
    }

    fn target(&self, session: (usize, usize)) -> Target {
        Target {
            session,
            id: self.id,
            revision: self.revision,
        }
    }
}

/// In-memory preview admission only; real queue ownership belongs to Node/ADK.
#[derive(Default)]
pub(crate) struct State {
    pub entries: Vec<Entry>,
    pub paused: bool,
    next_id: usize,
}

impl State {
    pub fn push(&mut self, text: SharedString, options: Options, owner: Owner) {
        self.entries.push(Entry {
            id: self.next_id,
            revision: 0,
            text,
            options,
            owner,
        });
        self.next_id += 1;
    }

    fn index(&self, target: Target) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.id == target.id && entry.revision == target.revision)
    }
}

impl Shell {
    pub(super) fn resume_queued_preview(
        &mut self,
        key: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .conversations
            .get(&key)
            .is_some_and(|thread| !thread.queue.paused)
            && self.begin_queued_preview(key, cx)
        {
            self.animate_preview(key, window, cx);
        }
    }

    pub(super) fn queued_options(&self, key: (usize, usize), cx: &App) -> Options {
        let mut options = self.conversations[&key].options.clone();
        options.references = super::references::active(
            &self.conversations[&key].input.read(cx).content(),
            &options.references,
        );
        if let Some((channel, model)) = self.selected_model_for(key, cx) {
            options.model = Some(Selection {
                channel: channel.id,
                model: model.id.clone(),
            });
            options.effort = options
                .effort
                .filter(|effort| {
                    *effort != sailry_protocol::Effort::Default && model.efforts.contains(effort)
                })
                .or(Some(sailry_protocol::Effort::initial(
                    &model.efforts,
                    model.default_effort,
                )));
        }
        options
    }

    fn queue_allowed(&self, key: (usize, usize), entry: &Entry) -> bool {
        self.workspace
            .sessions
            .get(&key)
            .is_some_and(|session| session.owner == entry.owner)
            && self
                .workspace
                .projects
                .get(&entry.owner.project)
                .is_some_and(|project| project.trusted)
    }

    pub(super) fn begin_queued_preview(
        &mut self,
        key: (usize, usize),
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(thread) = self.conversations.get(&key) else {
            return false;
        };
        if thread.turns.last().is_some_and(|turn| turn.status.active()) {
            return false;
        }
        let Some(entry) = thread.queue.entries.first() else {
            return false;
        };
        if !self.queue_allowed(key, entry) {
            return false;
        }
        let thread = self.conversations.get_mut(&key).unwrap();
        let entry = thread.queue.entries.remove(0);
        let mut turn = Turn::new(entry.text);
        turn.options = Some(entry.options);
        thread.turns.push(turn);
        thread.queue.paused = false;
        thread.scroller.update(cx, |state, cx| {
            state.append(1, cx);
            state.scroll_to_end(cx);
        });
        cx.notify();
        true
    }

    pub(crate) fn edit_queued(
        &mut self,
        target: Target,
        content: gpui_kit::base::input::InputContent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(thread) = self.conversations.get_mut(&target.session) else {
            return false;
        };
        let Some(index) = thread.queue.index(target) else {
            return false;
        };
        let entry = &mut thread.queue.entries[index];
        let text = content.text().clone();
        let references = super::references::active(&content, &entry.options.references);
        if text.trim().is_empty() && !entry.options.attachment && references.is_empty() {
            return false;
        }
        entry.text = text;
        entry.options.references = references;
        entry.revision += 1;
        cx.notify();
        true
    }

    pub(crate) fn remove_queued(&mut self, target: Target, cx: &mut Context<Self>) {
        if let Some(thread) = self.conversations.get_mut(&target.session)
            && let Some(index) = thread.queue.index(target)
        {
            thread.queue.entries.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn reorder_queued(
        &mut self,
        source: Target,
        destination: Target,
        cx: &mut Context<Self>,
    ) {
        if source.session != destination.session {
            return;
        }
        if let Some(thread) = self.conversations.get_mut(&source.session)
            && let Some(from) = thread.queue.index(source)
            && let Some(to) = thread.queue.index(destination)
        {
            let entry = thread.queue.entries.remove(from);
            thread.queue.entries.insert(to, entry);
            cx.notify();
        }
    }

    pub(crate) fn send_queued(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.conversations.get(&target.session) else {
            return;
        };
        let Some(index) = thread.queue.index(target) else {
            return;
        };
        if !self.queue_allowed(target.session, &thread.queue.entries[index]) {
            return;
        }
        self.stop_preview(target.session, cx);
        let queue = &mut self.conversations.get_mut(&target.session).unwrap().queue;
        let entry = queue.entries.remove(index);
        queue.entries.insert(0, entry);
        if self.begin_queued_preview(target.session, cx) {
            self.animate_preview(target.session, window, cx);
        }
    }
}
