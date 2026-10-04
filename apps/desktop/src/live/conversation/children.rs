use super::*;
use crate::{
    conversation::live::subagents::{Panel, Tab},
    resources::SideResource,
};

#[cfg(test)]
mod tests;

impl Shell {
    pub(super) fn open_live_child(
        &mut self,
        source: Entity<Chat>,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = source.read(cx).child_session(id) else {
            return;
        };
        self.show_live_child(source, session, window, cx);
    }

    pub(crate) fn show_live_child(
        &mut self,
        source: Entity<Chat>,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Conversation || self.current_chat() != Some(&source) {
            return;
        }
        let binding = source.read(cx).binding();
        let node = binding.client.target();
        if session.delegation.as_ref().map(|origin| origin.session) != source.read(cx).session()
            || session.delegation.is_none()
            || session.project != binding.project
        {
            return;
        }
        let id = session.id;
        let original = source.clone();
        let pending = session.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.show_live_child(original.clone(), pending.clone(), window, cx)
        }) {
            return;
        }
        if let Some(SideResource::Child(panel)) = &mut self.side_resource
            && panel.source == source
            && panel.select(id)
        {
            self.layout.panel_open[0] = true;
            self.mark_session_read(node, id, window, cx);
            cx.notify();
            return;
        }
        let mut binding = source.read(cx).binding();
        binding.project = session.project;
        binding.worktree = Some(session.worktree);
        binding.branch = String::new().into();
        let child = self.chat_view(binding, Some(session), window, cx);
        let tab = Tab {
            id,
            _observer: cx.observe(&child, |_, _, cx| cx.notify()),
            view: child,
        };
        if let Some(SideResource::Child(panel)) = &mut self.side_resource
            && panel.source == source
        {
            panel.tabs.push(tab);
            panel.select(id);
        } else {
            let observer = cx.observe(&source, |_, _, cx| cx.notify());
            self.side_resource = Some(SideResource::Child(Panel {
                source,
                selected: id,
                tabs: vec![tab],
                scroll: ScrollHandle::new(),
                _observer: observer,
            }));
        }
        self.layout.panel_open[0] = true;
        self.mark_session_read(node, id, window, cx);
        cx.notify();
    }

    pub(crate) fn select_live_child(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if let Some(SideResource::Child(panel)) = &mut self.side_resource
            && panel.select(id)
        {
            cx.notify();
        }
    }

    pub(crate) fn close_live_child(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(SideResource::Child(panel)) = &mut self.side_resource else {
            return;
        };
        panel.close(id);
        if panel.tabs.is_empty() {
            self.close_resource_panel(cx);
            self.focus.focus(window, cx);
        } else {
            self.panel_focus.focus(window, cx);
            cx.notify();
        }
    }
}
