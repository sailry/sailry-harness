//! Connection panels share session navigation; Node remains the history owner.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};

mod picker;

impl View {
    pub(crate) fn set_external_header(&mut self, external: bool, cx: &mut Context<Self>) {
        if self.external_header != external {
            self.external_header = external;
            cx.notify();
        }
    }

    pub(crate) fn session_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_1()
            .child(
                Button::new("connection-chat-new")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip(tr("chat_new"))
                    .accessibility_label(tr("chat_new"))
                    .debug_selector(|| "connection-chat-new".into())
                    .disabled(!self.can_switch_session())
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.switch_session(None, window, cx);
                    })),
            )
            .child(
                Button::new("connection-chat-history")
                    .ghost()
                    .small()
                    .icon(IconName::BookOpen)
                    .tooltip(tr("chat_connection_history"))
                    .accessibility_label(tr("chat_connection_history"))
                    .debug_selector(|| "connection-chat-history".into())
                    .disabled(!self.can_switch_session())
                    .on_click(cx.listener(|view, _, window, cx| {
                        if view.can_switch_session() {
                            picker::open(view, window, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn can_switch_session(&self) -> bool {
        self.sidebar
            && (self.resource.is_some() || self.assistant.is_some())
            && !self.busy()
            && self.connected()
    }

    pub(super) fn connection_sessions(&self) -> Vec<Session> {
        if self.resource.is_none() && self.assistant.is_none() {
            return Vec::new();
        }
        self.node
            .snapshot
            .iter()
            .flat_map(|snapshot| snapshot.sessions.iter())
            .filter(|session| {
                session.config.resource == self.resource
                    && session.config.assistant == self.assistant
                    && session.project == self.binding.project
                    && (self.binding.project.is_none()
                        || self.binding.worktree == Some(session.worktree))
                    && !session.archived
                    && session.delegation.is_none()
            })
            .cloned()
            .collect()
    }

    pub(super) fn switch_session(
        &mut self,
        id: Option<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_switch_session() || id.is_some() && id == self.session() {
            return;
        }
        let session = match id {
            Some(id) => match self
                .connection_sessions()
                .into_iter()
                .find(|session| session.id == id)
            {
                Some(session) => Some(session),
                None => return,
            },
            None => None,
        };
        let mut binding = self.binding.clone();
        if let Some(session) = &session {
            binding.project = session.project;
            binding.worktree = Some(session.worktree);
        }
        let sequence = self.release_draft_attachments();
        let mut next = if let Some(assistant) = self.assistant.clone() {
            Self::for_assistant(binding, session, assistant, self.resource, window, cx)
        } else {
            Self::for_connection(
                binding,
                session,
                self.resource.expect("connection sidebar"),
                self.composer_options,
                window,
                cx,
            )
        };
        if id.is_none() {
            next.config = self.config.clone().map(|mut config| {
                config.permission = sailry_protocol::Permission::Ask;
                config
            });
            next.config_owner = self
                .session
                .as_ref()
                .and_then(|session| session.profile.as_ref())
                .map_or(self.config_owner, |profile| profile.source);
        }
        next.hosts = self.hosts.clone();
        next.external_header = self.external_header;
        next.set_attachment_sequence(sequence);
        *self = next;
        self.focus(window, cx);
        cx.notify();
    }
}
