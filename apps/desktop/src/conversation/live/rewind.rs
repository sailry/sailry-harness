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

    pub(super) fn rewound(&mut self, backup: Session, window: &mut Window, cx: &mut Context<Self>) {
        self.backup = Some(backup.clone());
        let owner = cx.weak_entity();
        let close_owner = owner.clone();
        let id = backup.id;
        let message = gpui_kit::component::notification::Notification::success(tr("chat_rewound"))
            .id1::<Self>(id.to_string())
            .action(move |_, _, cx| {
                let owner = owner.clone();
                let backup = backup.clone();
                Button::new("rewind-backup")
                    .debug_selector(|| "live-rewind-backup".into())
                    .ghost()
                    .small()
                    .label(tr("chat_rewind_backup"))
                    .on_click(cx.listener(move |toast, _, window, cx| {
                        toast.dismiss(window, cx);
                        _ = owner
                            .update(cx, |_, cx| cx.emit(Event::Forked(Box::new(backup.clone()))));
                    }))
            })
            // Kit actions default to persistent notifications; rewind feedback expires.
            .autohide(true)
            .on_close(move |_, cx| {
                _ = close_owner.update(cx, |view, cx| {
                    if view.backup.as_ref().is_some_and(|backup| backup.id == id) {
                        view.backup = None;
                        cx.notify();
                    }
                });
            });
        crate::feedback::toast(window, tr("chat_rewound"), message, cx);
    }
}
