//! Submitted input is local presentation, not an admission or retry owner.
use super::*;
use gpui_kit::base::input::InputContent;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    notification::Notification,
};
use sailry_protocol::{RequestId, conversation::reference::Reference};
use std::ops::Range;

#[derive(Default)]
pub(super) struct State {
    pub(super) active: Option<Draft>,
    pub(super) failed: BTreeMap<RequestId, Draft>,
}

pub(super) struct Draft {
    pub(super) id: RequestId,
    identity: recovery::Identity,
    content: InputContent,
    selection: Range<usize>,
    references: Vec<Reference>,
    commands: BTreeMap<String, crate::plugins::contributions::Key>,
    attachments: Vec<attachments::Captured>,
    edited: bool,
}

impl State {
    pub(super) fn edited(&mut self) {
        if let Some(draft) = &mut self.active {
            draft.edited = true;
        }
    }
}

struct RecoveryToast;

impl View {
    pub(super) fn capture_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        self.submissions.active = Some(Draft {
            id: RequestId::new(),
            identity: self.recovery_identity(),
            content: input.content(),
            selection: input.selected_range(),
            references: self.references.selected.clone(),
            commands: self.references.command_keys.clone(),
            attachments: self.attachments.capture(),
            edited: false,
        });
        self.references.selected.clear();
        self.references.command_keys.clear();
        self.restored_input = None;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    pub(super) fn accept_submission(&mut self, ids: &[sailry_protocol::AttachmentId]) {
        if let Some(draft) = self.submissions.active.take() {
            self.attachments.sent_captured(&draft.attachments);
        } else {
            self.attachments.sent(ids);
        }
    }

    pub(super) fn bind_submission(&mut self) {
        let identity = self.recovery_identity();
        if let Some(draft) = &mut self.submissions.active {
            // Only the completion of this draft's own CreateSession binds it.
            // Retargeting is blocked while that captured request is pending.
            draft.identity = identity;
        }
    }

    pub(super) fn fail_submission(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.submissions.active.take() else {
            return;
        };
        if !draft.edited && self.can_restore_submission(&draft, cx) {
            self.restore_submission_input(draft, window, cx);
            return;
        }
        let id = draft.id;
        self.submissions.failed.insert(id, draft);
        // This toast owns recovery access independently of a later operation's
        // error state, so clearing that state cannot discard the failed input.
        self.error = None;
        let owner = cx.weak_entity();
        let close_owner = owner.clone();
        let note = Notification::error(tr(key))
            .id1::<RecoveryToast>((
                ElementId::from(("chat-submission", cx.entity_id())),
                SharedString::from(id.to_string()),
            ))
            .on_close(move |_, cx| {
                _ = close_owner.update(cx, |view, cx| {
                    view.submissions.failed.remove(&id);
                    cx.notify();
                });
            })
            .action(move |_, _, cx| {
                let owner = owner.clone();
                Button::new("chat-restore-draft")
                    .ghost()
                    .small()
                    .label(tr("chat_restore_draft"))
                    .debug_selector(|| "chat-restore-draft".into())
                    .on_click(cx.listener(move |toast, _, window, cx| {
                        if matches!(
                            owner.update(cx, |view, cx| view.restore_submission(id, window, cx)),
                            Ok(true)
                        ) {
                            toast.dismiss(window, cx);
                        }
                    }))
            });
        crate::feedback::toast(window, tr(key), note, cx);
        cx.notify();
    }

    fn can_restore_submission(&self, draft: &Draft, cx: &App) -> bool {
        let current = self.recovery_identity();
        current.node == draft.identity.node
            && current.project == draft.identity.project
            && current.worktree == draft.identity.worktree
            && current.resource == draft.identity.resource
            && current.assistant == draft.identity.assistant
            && draft
                .identity
                .session
                .is_none_or(|session| current.session == Some(session))
            && !self.readonly()
            && !self.busy()
            && self.input.read(cx).value().is_empty()
            && self.input.read(cx).tokens().is_empty()
            && self.active_references(cx).is_empty()
            && self.attachments.can_restore(&draft.attachments)
    }

    pub(super) fn restore_submission(
        &mut self,
        id: RequestId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(draft) = self.submissions.failed.get(&id) else {
            return false;
        };
        if !self.can_restore_submission(draft, cx) {
            let key = if !self.input.read(cx).value().is_empty()
                || !self.input.read(cx).tokens().is_empty()
                || !self.active_references(cx).is_empty()
            {
                "queue_draft_present"
            } else {
                "chat_restore_unavailable"
            };
            crate::feedback::toast(window, tr(key), Notification::error(tr(key)), cx);
            return false;
        }
        let draft = self.submissions.failed.remove(&id).unwrap();
        self.restore_submission_input(draft, window, cx);
        true
    }

    fn restore_submission_input(
        &mut self,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.attachments
            .restore_captured(draft.attachments, &self.stop);
        self.references.selected = draft.references;
        self.references.command_keys = draft.commands;
        self.restored_input = Some(draft.content.clone());
        self.input.update(cx, |input, cx| {
            input.set_value(draft.content, window, cx);
            input.set_selected_range(draft.selection, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }
}
