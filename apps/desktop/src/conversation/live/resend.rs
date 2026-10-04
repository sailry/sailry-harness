//! An explicit retry submits the original input as a new turn, preserving prior outcomes.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};

#[cfg(test)]
mod tests;

impl View {
    fn can_resend(&self, turn: TurnId) -> bool {
        if self.readonly() || !self.connected() || self.pending || self.editing.is_some() {
            return false;
        }
        if let Some(attempt) = &self.retry {
            // Recover uncertain admission with its original request ID, even if
            // the accepted turn has since started running on the Node.
            return attempt.is_resend(turn);
        }
        !self.busy()
            && self.configured()
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                let page = &snapshot.page;
                page.queue.items.is_empty()
                    && page.runs.iter().all(|run| {
                        !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                    })
                    && page.runs.iter().any(|run| {
                        run.turn == turn
                            && matches!(run.status, Status::Failed | Status::Interrupted)
                    })
                    && !messages::sent_input(page, turn).is_empty()
            })
    }

    fn resend(&mut self, turn: TurnId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_resend(turn) {
            return;
        }
        if self.retry.is_some() {
            self.retry(window, cx);
            return;
        }
        let Some(session) = &self.session else { return };
        let Some(snapshot) = &self.history.snapshot else {
            return;
        };
        self.execute(
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: messages::sent_input(&snapshot.page, turn),
            },
            actions::Action::Resend(turn),
            window,
            cx,
        );
    }

    pub(super) fn resend_button(&self, turn: TurnId, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new(format!("{turn}-retry"))
            .debug_selector(move || format!("live-turn-retry-{turn}"))
            .ghost()
            .small()
            .icon(IconName::RotateCw)
            .tooltip(tr("chat_retry"))
            .accessibility_label(tr("chat_retry"))
            .loading(
                self.pending
                    && self
                        .retry
                        .as_ref()
                        .is_some_and(|attempt| attempt.is_resend(turn)),
            )
            .disabled(!self.can_resend(turn))
            .on_click(cx.listener(move |view, _, window, cx| view.resend(turn, window, cx)))
    }
}
