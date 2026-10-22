use super::*;
use gpui_kit::component::{button::ButtonCustomVariant, separator::Separator};
use sailry_protocol::conversation::{
    Page, Run,
    compaction::{Failure, Source},
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
        let cause = failure
            .cause
            .as_deref()
            .filter(|cause| !cause.trim().is_empty())
            .map(diagnostic::display)
            .unwrap_or_else(|| tr("compact_reason_unknown").to_string());
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(Separator::horizontal().flex_1())
                    .child(
                        Button::new(id.clone())
                            .custom(ButtonCustomVariant::new(cx).foreground(cx.theme().danger))
                            .small()
                            .px_0()
                            .label(tr(if failure.source == Source::Automatic {
                                "compact_auto_failed"
                            } else {
                                "chat_compact_failed"
                            }))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.expand(turn, key.clone(), !open, cx)
                            })),
                    )
                    .child(Separator::horizontal().flex_1()),
            )
            .content(
                div()
                    .id(format!("{id}-scroll"))
                    .min_w_0()
                    .max_h_40()
                    .overflow_y_scroll()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(gpui_kit::base::SelectableText::new(
                        format!("{id}-text"),
                        cause,
                    )),
            )
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
