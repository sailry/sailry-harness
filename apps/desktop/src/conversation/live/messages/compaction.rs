use super::*;
use gpui_kit::component::separator::Separator;
use sailry_protocol::conversation::{
    Page, Run,
    compaction::{Failure, Reason, Source, Stage},
};

impl View {
    pub(super) fn compaction_turn(
        &self,
        run: &Run,
        page: &Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let turn = run.turn;
        let summaries = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| {
                entry
                    .parts
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, part)| {
                        if let Part::Compaction(text) = part {
                            Some((format!("{}-{index}", entry.id), text))
                        } else {
                            None
                        }
                    })
            })
            .collect::<Vec<_>>();
        let failures = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| {
                entry
                    .parts
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, part)| {
                        let Part::Resource(value) = part else {
                            return None;
                        };
                        Failure::from_resource(value)
                            .map(|failure| (format!("{}-{index}", entry.id), failure))
                    })
            })
            .collect::<Vec<_>>();
        let status = super::super::compaction::status(run, page);
        let row = v_flex()
            .id(format!("live-turn-{turn}"))
            .debug_selector(move || format!("live-turn-{turn}"))
            .w_full()
            .min_w_0()
            .max_w(px(super::super::super::CONTENT_WIDTH))
            .mx_auto()
            .px_6()
            .gap_2()
            .when(summaries.is_empty() && failures.is_empty(), |row| {
                row.child(
                    h_flex()
                        .id(format!("compact-status-{turn}"))
                        .when_some(run.error.as_ref(), |row, error| {
                            let detail = tr(failure_key(error));
                            row.tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(detail.clone())
                                    .build(window, cx)
                            })
                        })
                        .w_full()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .debug_selector(move || format!("live-compaction-status-{turn}-{status}"))
                        .child(Separator::horizontal().flex_1())
                        .child(if frame::active(run.status) {
                            ShimmerText::new(tr(status))
                                .id(format!("compact-{turn}"))
                                .into_any_element()
                        } else {
                            div().child(tr(status)).into_any_element()
                        })
                        .child(Separator::horizontal().flex_1()),
                )
            })
            .children(
                failures
                    .into_iter()
                    .map(|(key, failure)| self.compaction_failure(turn, key, &failure, cx)),
            )
            .children(
                summaries
                    .into_iter()
                    .map(|(key, text)| self.compaction(turn, key, text, cx)),
            );
        row.into_any_element()
    }

    pub(super) fn compaction_failure(
        &self,
        turn: TurnId,
        key: String,
        failure: &Failure,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("context-failure-{turn}-{key}");
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(false);
        let stage = match failure.stage {
            Stage::Unknown => "compact_stage_unknown",
            Stage::History => "compact_stage_history",
            Stage::Budget => "compact_stage_budget",
            Stage::Request => "compact_stage_request",
            Stage::Summary => "compact_stage_summary",
            Stage::Persistence => "compact_stage_persistence",
        };
        let (reason, suggestion) = match failure.reason {
            Reason::Unknown => ("compact_reason_unknown", "compact_suggest_retry"),
            Reason::HistoryMismatch => ("compact_reason_history", "compact_suggest_report"),
            Reason::NoFittingPrefix => ("compact_reason_budget", "compact_suggest_context"),
            Reason::RequestTimeout => ("compact_reason_timeout", "compact_suggest_retry"),
            Reason::RateLimited => ("compact_reason_rate", "compact_suggest_wait"),
            Reason::AuthorizationFailed => ("compact_reason_auth", "compact_suggest_auth"),
            Reason::ServiceRejected => ("compact_reason_rejected", "compact_suggest_retry"),
            Reason::EmptySummary => ("compact_reason_empty", "compact_suggest_retry"),
            Reason::RequestFailed => ("compact_reason_request", "compact_suggest_retry"),
            Reason::TruncatedSummary => ("compact_reason_truncated", "compact_suggest_context"),
            Reason::IncompleteResponse => ("compact_reason_incomplete", "compact_suggest_retry"),
            Reason::UnexpectedOutput => ("compact_reason_output", "compact_suggest_retry"),
            Reason::SummaryTooLarge => ("compact_reason_size", "compact_suggest_context"),
            Reason::NoReduction => ("compact_reason_reduction", "compact_suggest_context"),
            Reason::PersistenceFailed => ("compact_reason_persistence", "compact_suggest_retry"),
        };
        let status = self
            .history
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.page.runs.iter().find(|run| run.turn == turn))
            .map(|run| run.status);
        let outcome = if !failure.continued {
            "compact_task_stopped"
        } else {
            match status {
                Some(Status::Completed) => "compact_task_completed",
                Some(Status::Running | Status::Continued) => "compact_task_continued",
                _ => "compact_task_stopped",
            }
        };
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                crate::conversation::disclosure::trigger(
                    id.clone(),
                    IconName::CircleX,
                    tr(if failure.source == Source::Automatic {
                        "compact_auto_failed"
                    } else {
                        "chat_compact_failed"
                    }),
                    crate::conversation::disclosure::Detail {
                        failed: true,
                        ..Default::default()
                    },
                    open,
                    cx,
                )
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
            )
            .content(crate::conversation::disclosure::body(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(stage))
                    .child(tr(reason))
                    .when_some(failure.status, |body, status| {
                        body.child(format!("HTTP {status}"))
                    })
                    .child(tr(outcome))
                    .child(tr(suggestion))
                    .child(
                        Button::new(format!("{id}-retry"))
                            .small()
                            .label(tr("chat_retry"))
                            .disabled(!self.can_compact())
                            .on_click(
                                cx.listener(|view, _, window, cx| view.compact_context(window, cx)),
                            ),
                    ),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn compaction(
        &self,
        turn: TurnId,
        key: String,
        _text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("{turn}-{key}");
        let toggle = format!("live-context-{id}");
        h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(Separator::horizontal().flex_1())
            .child(crate::conversation::disclosure::summary(
                toggle,
                IconName::FileText,
                tr("chat_compacted"),
                crate::conversation::disclosure::Detail::default(),
                cx,
            ))
            .child(Separator::horizontal().flex_1())
            .into_any_element()
    }
}
