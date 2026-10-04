//! A composer draft shown while admission is pending, never a second history writer.
use super::*;
use gpui_kit::component::message::{Message as ChatMessage, MessageAlignment};
use sailry_protocol::{RequestId, conversation::Input};

pub(super) struct Message {
    pub row: TurnId,
    pub request: Option<RequestId>,
    pub turn: Option<TurnId>,
    pub input: Input,
    pub worktree: Option<WorktreeId>,
    pub attachments: Vec<attachments::images::ImageSource>,
}

impl View {
    pub(super) fn show_outgoing(&mut self, input: Input, cx: &mut Context<Self>) {
        self.outgoing = Some(Message {
            row: TurnId::new(),
            request: None,
            turn: None,
            input,
            worktree: self.binding.worktree,
            attachments: self.attachment_previews(),
        });
        self.sync_rows(cx);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        cx.notify();
    }

    pub(super) fn reconcile_outgoing(&mut self) {
        let Some(message) = &mut self.outgoing else {
            return;
        };
        if let Some(request) = message.request
            && let Some(turn) = self.node.snapshot.as_ref().and_then(|snapshot| {
                snapshot.turns.iter().find(|turn| {
                    turn.request == request
                        && Some(turn.session) == self.session.as_ref().map(|s| s.id)
                })
            })
        {
            message.turn = Some(turn.id);
        }
        let Some(turn) = message.turn else { return };
        // The admission snapshot or reply identifies the exact turn. Identical text
        // sent by another controller must never consume this local draft.
        if self.history.snapshot.as_ref().is_some_and(|snapshot| {
            (snapshot
                .page
                .queue
                .items
                .iter()
                .any(|item| item.turn == turn)
                && snapshot.page.runs.iter().any(|run| {
                    run.turn != turn && matches!(run.status, Status::Running | Status::Stopping)
                }))
                || (snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn && run.status != Status::Queued)
                    && snapshot
                        .page
                        .entries
                        .iter()
                        .any(|entry| entry.turn == turn && entry.author == "user"))
        }) {
            self.outgoing = None;
        }
    }

    pub(super) fn outgoing_message(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = &self.outgoing else {
            return div().into_any_element();
        };
        div()
            .debug_selector(|| "live-outgoing-message".into())
            .w_full()
            .min_w_0()
            .max_w(px(super::super::CONTENT_WIDTH))
            .mx_auto()
            .px(if self.sidebar { px(12.) } else { px(24.) })
            .child(
                ChatMessage::new()
                    .alignment(MessageAlignment::End)
                    .content(self.user_content(
                        message.row,
                        message.input.text.clone(),
                        &message.input.references,
                        message.attachments.clone(),
                        cx,
                    ))
                    .footer(self.user_footer(message.row, &message.input.text, None, cx)),
            )
            .into_any_element()
    }
}
