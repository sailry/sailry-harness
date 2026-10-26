//! Local input retained for presentation until canonical user content arrives.
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
    pub queue_revision: Option<u64>,
}

impl View {
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
            queue_revision: None,
        });
        self.sync_rows(cx);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        cx.notify();
    }

    pub(super) fn reconcile_outgoing(&mut self) {
        if let Some(snapshot) = &self.history.snapshot {
            self.sent.retain(|turn, message| {
                !snapshot
                    .page
                    .entries
                    .iter()
                    .any(|entry| entry.turn == *turn && entry.author == "user")
                    && snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.turn == *turn && run.status != Status::Cancelled)
                    && snapshot
                        .page
                        .queue
                        .items
                        .iter()
                        .find(|item| item.turn == *turn)
                        .is_none_or(|item| {
                            Some(item.revision) == message.queue_revision && item.guidance.is_none()
                        })
            });
        }
        let Some(message) = &mut self.outgoing else {
            return;
        };
        if let Some(request) = message.request {
            let pending = self.history.snapshot.as_ref().and_then(|snapshot| {
                sailry_client::conversation::turns::pending(&snapshot.page.queue, request)
            });
            if let Some(item) = pending {
                message.turn = Some(item.turn);
                if self.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.queue.paused
                        || !item.ready
                        || item.guidance.is_some()
                        || snapshot.page.runs.iter().any(|run| {
                            run.turn != item.turn
                                && matches!(run.status, Status::Running | Status::Stopping)
                        })
                }) {
                    message.queued = true;
                }
            } else if let Some(turn) = self.node.snapshot.as_ref().and_then(|snapshot| {
                snapshot.turns.iter().find(|turn| {
                    turn.request == request
                        && Some(turn.session) == self.session.as_ref().map(|s| s.id)
                })
            }) {
                message.turn = Some(turn.id);
            }
        }
        let Some(turn) = message.turn else { return };
        // The admission snapshot or reply identifies the exact turn. Identical text
        // sent by another controller must never consume this local draft.
        if self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .page
                .entries
                .iter()
                .any(|entry| entry.turn == turn && entry.author == "user")
        }) {
            self.outgoing = None;
        } else if message.queued
            && let Some(item) = self.history.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .page
                    .queue
                    .items
                    .iter()
                    .find(|item| item.turn == turn)
            })
        {
            message.queue_revision = Some(item.revision);
            // Durable queue admission frees the composer without losing the full
            // input needed if execution precedes the canonical user event.
            self.sent.insert(turn, self.outgoing.take().unwrap());
        }
    }

    /// The local immediate submission already represented in the transcript.
    pub(super) fn transcript_pending(&self) -> Option<&sailry_protocol::conversation::Pending> {
        let message = self.outgoing.as_ref().filter(|message| !message.queued)?;
        let snapshot = self.history.snapshot.as_ref()?;
        let item =
            sailry_client::conversation::turns::pending(&snapshot.page.queue, message.request?)?;
        (!snapshot.page.queue.paused
            && item.ready
            && item.guidance.is_none()
            && !snapshot.page.runs.iter().any(|run| {
                run.turn != item.turn && matches!(run.status, Status::Running | Status::Stopping)
            }))
        .then_some(item)
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
