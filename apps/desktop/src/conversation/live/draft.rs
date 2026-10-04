//! Draft state stays in the desktop view until the first send establishes a session.
use super::*;

impl View {
    pub(crate) fn can_retarget(&self) -> bool {
        self.resource.is_none()
            && self.assistant.is_none()
            && self.session.is_none()
            && !self.busy()
            && self.attachments.can_retarget()
    }

    pub(super) fn busy(&self) -> bool {
        self.pending || self.retry.is_some() || self.preparing.is_some()
    }

    pub(super) fn saving_config(&self) -> bool {
        self.pending
            && self
                .retry
                .as_ref()
                .is_some_and(|attempt| attempt.is_configure())
    }

    pub(crate) fn set_hosts(&mut self, hosts: Vec<(NodeId, SharedString)>, cx: &mut Context<Self>) {
        if self.hosts != hosts {
            self.hosts = hosts;
            cx.notify();
        }
    }

    pub(crate) fn retarget(
        &mut self,
        binding: Binding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_retarget() {
            return;
        }
        if self.binding.client.target() == binding.client.target()
            && self.binding.project == binding.project
            && self.binding.worktree == binding.worktree
        {
            return;
        }
        let sources = self.attachment_sources();
        let next_attachment = self.release_draft_attachments();
        let mut next = Self::with_input(binding, None, Some(self.input.clone()), window, cx);
        next.sidebar = self.sidebar;
        next.composer_options = self.composer_options;
        if next.binding.client.target() == self.binding.client.target() {
            next.config = self.config.clone();
            next.config_owner = self.config_owner;
            next.draft_mode = self.draft_mode;
            next.draft_permission = self.draft_permission;
        }
        next.hosts = self.hosts.clone();
        next.references.selected = self.references.selected.clone();
        // Command provenance must not be rebound by selecting another execution Node.
        next.inherit_command_keys(&self.references);
        next.set_attachment_sequence(next_attachment);
        *self = next;
        self.attach_sources(sources, window, cx);
        cx.notify();
    }
}
