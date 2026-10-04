//! Session read acknowledgements target the execution Node across all controllers.
use super::*;
use crate::tr;

impl Shell {
    pub(crate) fn current_session_is_visible(
        &self,
        node: NodeId,
        id: SessionId,
        window: &Window,
    ) -> bool {
        window.is_window_active()
            && self.page == Page::Conversation
            && self.session_scope.active == Key::Session(node, id)
    }

    pub(crate) fn read_current_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Key::Session(node, id) = self.session_scope.active
            && self.current_session_is_visible(node, id, window)
        {
            // Session dots and notification inboxes have separate read state.
            self.set_session_read(node, id, true, window, cx);
        }
    }

    fn session_attention(
        &self,
        node: NodeId,
        id: sailry_protocol::SessionId,
    ) -> Option<sailry_protocol::activity::Attention> {
        self.activity_snapshot(node)?
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.activity.attention)
    }

    pub(crate) fn session_unread(&self, node: NodeId, id: sailry_protocol::SessionId) -> bool {
        self.session_attention(node, id)
            .is_some_and(|attention| attention.unread)
    }

    pub(crate) fn mark_session_read(
        &mut self,
        node: NodeId,
        id: sailry_protocol::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_session_read(node, id, true, window, cx);
        self.activity.read_session(node, id);
    }

    pub(crate) fn set_session_read(
        &mut self,
        node: NodeId,
        id: sailry_protocol::SessionId,
        read: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(attention) = self.session_attention(node, id) else {
            return;
        };
        if attention.unread != read {
            return;
        }
        let Some(live) = &self.live else { return };
        let Some(transport) = live.transport_for(node) else {
            return;
        };
        let client = live.client_for(transport);
        let request = client.prepare(sailry_protocol::Command::SetSessionRead {
            session: id,
            expected_revision: attention.revision,
            read,
        });
        let job = cx
            .global::<crate::backend::Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            let _ = shell.update_in(cx, |_, window, cx| {
                if !matches!(result, Ok(Ok(_))) {
                    crate::feedback::toast(
                        window,
                        tr("live_request_failed"),
                        gpui_kit::component::notification::Notification::error(tr(
                            "live_request_failed",
                        )),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }
}
