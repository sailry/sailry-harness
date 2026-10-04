use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};

#[cfg(test)]
mod tests;

impl View {
    pub(super) fn rewind_request(&self, through: TurnId) -> Option<Command> {
        if self.pending || self.retry.is_some() || !self.connected() {
            return None;
        }
        let session = self.session()?;
        let page = &self.history.snapshot.as_ref()?.page;
        if !page.queue.items.is_empty()
            || page.runs.iter().any(|run| {
                matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
            })
        {
            return None;
        }
        let head = page.runs.last()?.turn;
        if head == through || !page.runs.iter().any(|run| run.turn == through) {
            return None;
        }
        Some(Command::RewindConversation {
            session,
            through: Some(through),
            expected_head: head,
            expected_revision: page.revision,
        })
    }

    pub(super) fn confirm_rewind(&self, turn: TurnId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self.rewind_request(turn) else {
            return;
        };
        let owner = cx.entity().downgrade();
        crate::prompts::confirm(
            &tr("chat_rewind"),
            &tr("chat_rewind_confirm"),
            tr("chat_rewind_action"),
            window,
            cx,
            move |window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    if view.rewind_request(turn).as_ref() == Some(&command) {
                        view.execute(command, actions::Action::Rewind(turn), window, cx);
                    } else {
                        view.error = Some("chat_history_changed");
                        cx.notify();
                    }
                });
            },
        );
    }

    pub(super) fn rewind_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let backup = self.backup.as_ref()?.clone();
        Some(
            h_flex()
                .debug_selector(|| "live-rewind-notice".into())
                .gap_2()
                .flex_wrap()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr("chat_rewound"))
                .child(
                    Button::new("rewind-backup")
                        .debug_selector(|| "live-rewind-backup".into())
                        .ghost()
                        .small()
                        .rounded_full()
                        .label(tr("chat_rewind_backup"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(Event::Forked(Box::new(backup.clone())))
                        })),
                )
                .child(
                    Button::new("rewind-dismiss")
                        .debug_selector(|| "live-rewind-dismiss".into())
                        .ghost()
                        .small()
                        .rounded_full()
                        .icon(IconName::Close)
                        .accessibility_label(tr("close"))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.backup = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}
