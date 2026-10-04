use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

#[cfg(test)]
mod tests;

impl View {
    pub(super) fn can_fork(&self, turn: TurnId) -> bool {
        !self.pending
            && self.retry.is_none()
            && self.connected()
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.runs.iter().any(|run| {
                    run.turn == turn
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                })
            })
    }

    fn fork(&mut self, turn: TurnId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_fork(turn) {
            return;
        }
        let Some(session) = &self.session else { return };
        self.execute(
            Command::ForkConversation {
                session: session.id,
                through: turn,
                expected_revision: session.revision,
            },
            actions::Action::Fork(turn),
            window,
            cx,
        );
    }

    pub(super) fn turn_menu(&self, turn: TurnId, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let pending = self.pending
            && self
                .retry
                .as_ref()
                .is_some_and(|attempt| attempt.is_fork(turn) || attempt.is_rewind(turn));
        let button = Button::new(format!("{turn}-more"))
            .debug_selector(move || format!("live-turn-more-{turn}"))
            .ghost()
            .small()
            .icon(IconName::Network)
            .loading(pending)
            .disabled(pending)
            .tooltip(tr("chat_fork"))
            .accessibility_label(tr("chat_fork"));
        button
            .dropdown_menu(move |menu, _, cx| {
                let disabled = owner
                    .read_with(cx, |view, _| !view.can_fork(turn))
                    .unwrap_or(true);
                let rewind_disabled = owner
                    .read_with(cx, |view, _| view.rewind_request(turn).is_none())
                    .unwrap_or(true);
                let create_disabled = owner
                    .read_with(cx, |view, cx| {
                        !view.git
                            || !view.can_move()
                            || !view.can_invoke_contribution(
                                sailry_protocol::plugin::ui::Intent::CreateWorktree,
                                cx,
                            )
                    })
                    .unwrap_or(true);
                let worktree = owner.clone();
                let branch = owner.clone();
                let rewind = owner.clone();
                menu.item(
                    PopupMenuItem::new(tr("chat_fork"))
                        .icon(IconName::Network)
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            let _ = branch.update(cx, |view, cx| view.fork(turn, window, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(tr("chat_rewind"))
                        .icon(IconName::Undo)
                        .disabled(rewind_disabled)
                        .on_click(move |_, window, cx| {
                            let _ =
                                rewind.update(cx, |view, cx| view.confirm_rewind(turn, window, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(tr("worktree_create"))
                        .icon(IconName::Plus)
                        .disabled(create_disabled)
                        .on_click(move |_, window, cx| {
                            let _ =
                                worktree.update(cx, |view, cx| view.create_worktree(window, cx));
                        }),
                )
            })
            .into_any_element()
    }
}
