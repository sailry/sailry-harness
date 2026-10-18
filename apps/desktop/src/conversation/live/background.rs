//! Completion notices render shared Node state without scheduling work from the UI.
use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    collapsible::Collapsible,
    popover::Popover,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::conversation::background::{Kind, Notice, Outcome, State};

fn outcome(value: Outcome) -> &'static str {
    match value {
        Outcome::Completed => "background_outcome_completed",
        Outcome::Failed => "background_outcome_failed",
        Outcome::Cancelled => "background_outcome_cancelled",
        Outcome::Stopped => "background_outcome_stopped",
        Outcome::TimedOut => "background_outcome_timed_out",
        Outcome::Interrupted => "background_outcome_interrupted",
        Outcome::Unknown => "background_outcome_unknown",
    }
}

impl View {
    fn background_item(&self, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let kind = notice.kind.clone();
        let actionable = match &kind {
            Kind::Agent { session, .. } => self.node.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.sessions.iter().any(|entry| {
                    entry.id == *session
                        && entry
                            .delegation
                            .as_ref()
                            .is_some_and(|origin| Some(origin.session) == self.session())
                })
            }),
            Kind::Command { id } => self.commands.read(cx).contains(*id),
        };
        v_flex()
            .gap_1()
            .min_w_0()
            .child(
                Button::new(SharedString::from(format!("notice-{}", notice.id)))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .disabled(!actionable)
                    .icon(match &kind {
                        Kind::Agent { .. } => IconName::Bot,
                        Kind::Command { .. } => IconName::SquareTerminal,
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(notice.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(outcome(notice.outcome))),
                            ),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| match &kind {
                        Kind::Agent { session, .. } => cx.emit(Event::Subagent(*session)),
                        Kind::Command { id } => view.open_background_command(*id, cx),
                    })),
            )
            .when_some(notice.summary.clone(), |row, summary| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(summary),
                )
            })
            .into_any_element()
    }

    pub(super) fn background_notices(
        &self,
        turn: TurnId,
        notices: &[Notice],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if notices.is_empty() {
            return None;
        }
        let id = notices[0].id.clone();
        let open = self
            .expanded
            .get(&(turn, format!("background-{id}")))
            .copied()
            .unwrap_or(true);
        Some(
            Collapsible::new()
                .open(open)
                .w_full()
                .min_w_0()
                .child(
                    Button::new(SharedString::from(format!("background-notices-{id}")))
                        .ghost()
                        .small()
                        .icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .label(tr("background_notice_title"))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            // Disclosure state belongs to the presentation, not the completion record.
                            view.expand(turn, format!("background-{id}"), !open, cx);
                        })),
                )
                .content(
                    v_flex().gap_2().children(
                        notices
                            .iter()
                            .map(|notice| self.background_item(notice, cx)),
                    ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn background_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.history.snapshot.as_ref()?;
        let notices: Vec<_> = snapshot
            .page
            .background
            .iter()
            .filter(|task| task.state == State::Pending && task.delivery.is_none())
            .filter_map(|task| {
                Some(Notice {
                    id: task.id.clone(),
                    kind: task.kind.clone(),
                    title: task.title.clone(),
                    outcome: task.outcome?,
                    summary: task.summary.clone(),
                })
            })
            .collect();
        if notices.is_empty() {
            return None;
        }
        let count = notices.len();
        let owner = cx.weak_entity();
        Some(
            Popover::new("pending-background")
                .anchor(Anchor::BottomLeft)
                .trigger(
                    Button::new("pending-background")
                        .outline()
                        .small()
                        .rounded_full()
                        .icon(IconName::Bot)
                        .label(rust_i18n::t!("background_pending", count = count).to_string()),
                )
                .content(move |_, window, cx| {
                    owner
                        .update(cx, |view, cx| {
                            div()
                                .w(px(400.).min(window.viewport_size().width - px(48.)))
                                .max_h(window.viewport_size().height - px(48.))
                                .min_h_0()
                                .child(crate::conversation::surface::scroll(
                                    "pending-background-content".into(),
                                    v_flex().gap_2().children(
                                        notices
                                            .iter()
                                            .map(|notice| view.background_item(notice, cx)),
                                    ),
                                ))
                                .into_any_element()
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .into_any_element(),
        )
    }
}
