//! Automatic turns show the finished background work that started them.
use super::*;
use sailry_protocol::conversation::{
    Run,
    background::{Kind, Notice, Outcome},
};

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

fn row(notice: &Notice, cx: &App) -> AnyElement {
    let failed = matches!(
        notice.outcome,
        Outcome::Failed | Outcome::TimedOut | Outcome::Interrupted | Outcome::Unknown
    );
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(
            Icon::new(match notice.kind {
                Kind::Agent { .. } => IconName::Bot,
                _ => IconName::SquareTerminal,
            })
            .size_4()
            .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(notice.title.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(if failed {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                })
                .child(tr(outcome(notice.outcome))),
        )
        .into_any_element()
}

/// Replaces the absent user bubble for turns delivered by background work.
pub(super) fn message(runs: &[&Run], cx: &App) -> Option<Message> {
    let delivered: Vec<_> = runs.iter().filter(|run| run.notice).collect();
    let notices: Vec<_> = delivered.iter().flat_map(|run| &run.notices).collect();
    let turn = delivered.first()?.turn;
    if notices.is_empty() {
        return None;
    }
    Some(
        Message::new().alignment(MessageAlignment::End).content(
            MessageContent::new().bubble(crate::conversation::surface::user_bubble(
                crate::theme::messages::user(cx).child(
                    v_flex()
                        .id(format!("background-notice-{turn}"))
                        .aria_label(tr("background_notice_title"))
                        .gap_1p5()
                        .children(notices.into_iter().map(|notice| row(notice, cx))),
                ),
            )),
        ),
    )
}
