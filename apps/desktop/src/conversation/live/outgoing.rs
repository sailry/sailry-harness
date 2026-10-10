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
    /// Sent while a turn runs; the queue tray shows it instead of the transcript.
    pub queued: bool,
    pub guidance: Option<Guidance>,
}

pub(super) struct Guidance {
    pub recipient: TurnId,
    /// Queue revision in the durable selection receipt; older views are not authority.
    pub revision: Option<u64>,
}

impl View {
    pub(super) fn show_guidance(
        &mut self,
        turn: TurnId,
        recipient: TurnId,
        request: RequestId,
        input: Input,
        attachments: Vec<sailry_protocol::attachment::Attachment>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.busy() || self.outgoing.is_some() {
            return false;
        }
        self.outgoing = Some(Message {
            row: turn,
            request: Some(request),
            turn: Some(turn),
            input,
            worktree: self.turn_worktree(turn),
            attachments: attachments.into_iter().map(Into::into).collect(),
            queued: false,
            guidance: Some(Guidance {
                recipient,
                revision: None,
            }),
        });
        self.sync_rows(cx);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        cx.notify();
        true
    }

    pub(super) fn show_outgoing(&mut self, input: Input, cx: &mut Context<Self>) {
        let queued = self.active().is_some();
        self.outgoing = Some(Message {
            row: TurnId::new(),
            request: None,
            turn: None,
            input,
            worktree: self.binding.worktree,
            attachments: self.attachment_previews(),
            queued,
            guidance: None,
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
        let queued = message.queued;
        let guidance = message
            .guidance
            .as_ref()
            .map(|guidance| (guidance.recipient, guidance.revision));
        // The admission snapshot or reply identifies the exact turn. Identical text
        // sent by another controller must never consume this local draft.
        if self.history.snapshot.as_ref().is_some_and(|snapshot| {
            guidance.is_some_and(|(recipient, revision)| {
                revision.is_some_and(|revision| snapshot.page.queue.revision >= revision)
                    && (snapshot
                        .page
                        .queue
                        .items
                        .iter()
                        .any(|item| item.turn == turn && item.guidance != Some(recipient))
                        || snapshot
                            .page
                            .runs
                            .iter()
                            .any(|run| run.turn == turn && run.status == Status::Cancelled))
            }) || (snapshot
                .page
                .queue
                .items
                .iter()
                .any(|item| item.turn == turn)
                && queued
                && (self.readonly()
                    || snapshot.page.runs.iter().any(|run| {
                        run.turn != turn && matches!(run.status, Status::Running | Status::Stopping)
                    })))
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

    /// Input waiting for admission while another turn runs.
    pub(super) fn queued_outgoing(&self) -> Option<SharedString> {
        let message = self.outgoing.as_ref().filter(|message| message.queued)?;
        let admitted = message.turn.is_some_and(|turn| {
            self.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .queue
                    .items
                    .iter()
                    .any(|item| item.turn == turn)
            })
        });
        if admitted {
            return None;
        }
        Some(if message.input.text.trim().is_empty() {
            rust_i18n::t!(
                "chat_attachment_count",
                count = message.input.attachments.len()
            )
            .to_string()
            .into()
        } else {
            message.input.text.trim().to_owned().into()
        })
    }

    pub(super) fn outgoing_message(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.outgoing.as_ref().filter(|message| !message.queued) else {
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
