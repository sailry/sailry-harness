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
        .text_sm()
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
    let notices: Vec<_> = runs
        .iter()
        .filter(|run| run.notice)
        .flat_map(|run| &run.notices)
        .collect();
    if notices.is_empty() {
        return None;
    }
    Some(
        Message::new().alignment(MessageAlignment::End).content(
            MessageContent::new().bubble(
                Bubble::new().with_variant(BubbleVariant::Outline).child(
                    v_flex()
                        .min_w(px(240.))
                        .max_w(px(480.))
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("background_notice_title")),
                        )
                        .children(notices.into_iter().map(|notice| row(notice, cx))),
                ),
            ),
        ),
    )
}
