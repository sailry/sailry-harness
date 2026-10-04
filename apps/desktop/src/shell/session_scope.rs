//! Focused conversation scope and parked resource views; pane membership lives in Workspaces.
mod attention;
mod panels;
#[cfg(test)]
mod tests;

use super::Shell;
use crate::preview::Page;
use gpui_kit::*;
use sailry_protocol::{NodeId, SessionId};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Key {
    #[default]
    Draft,
    Session(NodeId, SessionId),
    Preview(usize, usize),
}

impl Key {
    pub(crate) fn selector(self) -> String {
        match self {
            Self::Draft => "session-tab-draft".into(),
            Self::Session(node, session) => {
                format!("session-tab-{}-{session}", crate::live::short_id(node))
            }
            Self::Preview(host, session) => format!("session-tab-preview-{host}-{session}"),
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) active: Key,
    pub(crate) open: Vec<Key>,
    mounted: Option<Key>,
    pub(crate) panels: BTreeMap<Key, panels::Panel>,
}

impl State {
    pub(crate) fn open(&mut self, key: Key) {
        if key != Key::Draft && !self.open.contains(&key) {
            self.open.push(key);
        }
        self.active = key;
    }

    pub(crate) fn promote(&mut self, key: Key) {
        if !self.open.contains(&key) {
            self.open.push(key);
        }
        if self.active == Key::Draft {
            self.active = key;
        }
        if self.mounted == Some(Key::Draft) {
            self.mounted = Some(key);
        }
        if let Some(panel) = self.panels.remove(&Key::Draft) {
            self.panels.insert(key, panel);
        }
    }
}

impl Shell {
    pub(crate) fn start_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.is_some() {
            self.new_live_conversation(window, cx);
        } else if let Some(owner) = self.workspace.selected_owner(self.host) {
            self.create_session(owner, window, cx);
        } else {
            self.project_editor(self.host, None, window, cx);
        }
    }

    pub(crate) fn activate_session(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Key::Preview(host, session) = key {
            self.select_session((host, session), window, cx);
            return;
        }
        if let Key::Session(node, id) = key
            && (!self.chats.views.contains_key(&(node, id))
                || self
                    .live
                    .as_ref()
                    .is_none_or(|live| live.transport_for(node).is_none()))
        {
            return;
        }
        if let Key::Session(node, id) = key {
            crate::preferences::recent::record(node, id, cx);
            self.mark_session_read(node, id, window, cx);
        }
        self.park_session_panel(window, cx);
        self.session_scope.open(key);
        self.restore_session_scope(cx);
        self.page = Page::Conversation;
        self.open_session_pane(key, window, cx);
        self.sync_session_panel(window, cx);
        if let Some(chat) = self.current_chat() {
            chat.update(cx, |chat, cx| chat.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn close_session_view(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live.is_some() {
            let target = match key {
                Key::Session(node, id) => crate::panes::Target::Session(node, id),
                _ => crate::panes::Target::Draft,
            };
            self.remove_pane(target, window, cx);
            return;
        }
        let Some(index) = self.session_scope.open.iter().position(|item| *item == key) else {
            return;
        };
        if self.session_scope.active == key {
            self.park_session_panel(window, cx);
        }
        self.session_scope.open.remove(index);
        if self.session_scope.active == key {
            if let Some(next) = self
                .session_scope
                .open
                .get(index.min(self.session_scope.open.len().saturating_sub(1)))
                .copied()
            {
                self.activate_session(next, window, cx);
            } else {
                self.new_session_view(window, cx);
                self.session_scope.open.clear();
            }
        }
        cx.notify();
    }

    fn new_session_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.is_some() {
            self.new_live_conversation(window, cx);
        } else {
            let owner = self.workspace.owner(self.host);
            let empty = self
                .conversations
                .iter()
                .find(|(_, view)| view.turns.is_empty())
                .map(|(key, _)| *key);
            if let Some(key) = empty {
                self.select_session(key, window, cx);
            } else {
                self.create_session(owner, window, cx);
            }
        }
    }
}
