//! Binds desktop panes to existing session/terminal views and their execution scope.
use super::*;
use crate::panes::{self, Target};
use session_scope::Key;

pub(super) mod recovery;

impl Shell {
    pub(crate) fn open_session_pane(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live.is_none() {
            return;
        }
        let target = match key {
            Key::Draft => {
                // The existing draft owner initializes the composer and its host choices.
                self.draft_view(window, cx);
                Target::Draft
            }
            Key::Session(node, id) => Target::Session(node, id),
            Key::Preview(..) => return,
        };
        let Some(view) = self.current_chat().cloned() else {
            return;
        };
        let title = view
            .read(cx)
            .summary()
            .map(crate::activity::title)
            .unwrap_or_else(|| tr("chat_new"));
        let project = view.read(cx).binding().project_name;
        self.splits.update(cx, |splits, cx| {
            splits.open(target, title, project, view.into(), window, cx)
        });
    }

    pub(crate) fn focus_chat_pane(
        &mut self,
        view: &Entity<crate::conversation::live::View>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = if let Some(session) = view.read(cx).summary() {
            Target::Session(view.read(cx).binding().client.target(), session.id)
        } else {
            Target::Draft
        };
        self.splits
            .update(cx, |splits, cx| splits.focus(target, cx));
        self.focus_pane(target, window, cx);
    }

    fn focus_pane(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_plugin_focus(Some(target), cx);
        self.park_session_panel(window, cx);
        match target {
            Target::Draft | Target::Session(..) => {
                let key = match target {
                    Target::Session(node, id) => {
                        crate::preferences::recent::record(node, id, cx);
                        Key::Session(node, id)
                    }
                    _ => Key::Draft,
                };
                self.session_scope.open(key);
                self.restore_session_scope(cx);
                self.page = Page::Conversation;
            }
            Target::Terminal(node, tree, terminal) => {
                self.page = Page::Terminal;
                self.restore_terminal_scope(node, tree, terminal, cx);
            }
        }
        self.read_current_session(window, cx);
        self.sync_session_panel(window, cx);
        cx.notify();
    }

    pub(crate) fn activate_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane_content(target, window, cx);
        if matches!(target, Target::Terminal(..)) {
            self.enter_plugin_pane(target, window, cx);
        }
    }

    pub(crate) fn focus_pane_content(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane(target, window, cx);
        match target {
            Target::Terminal(node, _, id) => {
                self.focus_terminal_pane(node, id, window, cx);
            }
            _ => {
                if let Some(view) = self.current_chat() {
                    view.update(cx, |view, cx| view.focus(window, cx));
                }
            }
        }
    }

    pub(super) fn split_event(
        &mut self,
        event: &panes::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            panes::Event::Focus(target) => {
                if matches!(self.page, Page::Conversation | Page::Terminal)
                    && self.splits.read(cx).active == Some(*target)
                {
                    self.focus_pane(*target, window, cx);
                }
            }
            panes::Event::Activate(target) => {
                self.activate_pane(*target, window, cx);
            }
            panes::Event::RequestClose(target) => self.confirm_close_pane(*target, window, cx),
            panes::Event::Close(target) => self.close_pane(*target, window, cx),
            panes::Event::Drop {
                workspace,
                item,
                target,
            } => {
                let placement = target.placement();
                if !self
                    .splits
                    .read(cx)
                    .can_drop(*workspace, item.target, placement, cx)
                {
                    return;
                }
                if !self.mount_drag(item, window, cx) {
                    return;
                }
                self.splits.update(cx, |splits, cx| {
                    splits.drop(*workspace, item.target, *target, window, cx);
                });
                self.activate_pane(item.target, window, cx);
            }
            panes::Event::Changed => {
                self.splits.update(cx, |splits, cx| splits.persist(cx));
                cx.notify();
            }
        }
    }

    fn mount_drag(
        &mut self,
        item: &panes::Drag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.splits.read(cx).contains(item.target) {
            let node = match item.target {
                Target::Session(node, _) | Target::Terminal(node, ..) => node,
                _ => return false,
            };
            let Some(live) = &mut self.live else {
                return false;
            };
            if live.transport_for(node).is_none() {
                return false;
            }
            if live.selected != node {
                live.select(node, cx);
            }
            if let Some(session) = &item.session {
                self.reveal_session(session.clone(), window, cx);
            } else if let Some(info) = &item.terminal {
                self.reveal_terminal(item.project, info, window, cx);
            }
        }
        self.splits.read(cx).contains(item.target)
    }

    pub(crate) fn append_split(
        &mut self,
        destination: Target,
        item: &panes::Drag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .splits
            .read(cx)
            .can_append(destination, item.target, cx)
            || !self.mount_drag(item, window, cx)
        {
            return;
        }
        if self.splits.update(cx, |splits, cx| {
            splits.append(destination, item.target, window, cx)
        }) {
            self.sidebar.open_group(destination);
            self.activate_pane(item.target, window, cx);
        }
    }

    pub(crate) fn remove_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recovery.cancel(target);
        let visible = self.page == Page::Conversation
            || self.page == Page::Terminal && self.terminal_workspace_visible(cx);
        self.release_plugin_pane(target);
        if let Target::Session(node, id) = target
            && self.splits.read(cx).contains(target)
            && let Some(view) = self.chats.views.get(&(node, id))
        {
            view.update(cx, |view, cx| view.stop_background(window, cx));
        }
        let active = visible && self.splits.read(cx).active == Some(target);
        if active {
            self.park_session_panel(window, cx);
        }
        self.splits
            .update(cx, |splits, cx| splits.close(target, window, cx));
        if let Target::Session(node, id) = target {
            self.session_scope
                .open
                .retain(|key| *key != Key::Session(node, id));
        }
        if active {
            if let Some(next) = self.splits.read(cx).active {
                self.focus_pane_content(next, window, cx);
            } else {
                self.session_scope.active = Key::Draft;
                self.page = Page::Conversation;
                self.sync_session_panel(window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn split_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.page == Page::Conversation {
            let key = self.session_scope.active;
            let target = match key {
                Key::Session(node, id) => Target::Session(node, id),
                _ => Target::Draft,
            };
            if !self.splits.read(cx).contains(target) {
                self.open_session_pane(key, window, cx);
            } else if key == Key::Draft {
                // Keep the welcome composer's existing host/snapshot binding refreshed.
                self.draft_view(window, cx);
            }
        }
        for ((node, id), view) in &self.chats.views {
            self.splits.update(cx, |splits, cx| {
                splits.controls(
                    Target::Session(*node, *id),
                    view.read(cx).header_controls().into(),
                    cx,
                )
            });
            if let Some(summary) = view.read(cx).summary() {
                let title = crate::activity::title(summary);
                self.splits.update(cx, |splits, cx| {
                    splits.title(Target::Session(*node, *id), title, cx)
                });
            }
        }
        if let Some(view) = self
            .current_chat()
            .filter(|_| self.session_scope.active == Key::Draft)
        {
            self.splits.update(cx, |splits, cx| {
                splits.controls(Target::Draft, view.read(cx).header_controls().into(), cx)
            });
        }
        self.splits.clone().into_any_element()
    }
}
