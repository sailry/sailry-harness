use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::{Input as TextInput, Textarea},
    popover::Popover,
    radio::{Radio, RadioGroup},
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(in crate::conversation::live) fn pending_question(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editing = self.questions.editing.as_ref()?;
        let id = editing.id;
        let owner = cx.weak_entity();
        Some(
            crate::theme::popover(Popover::new(format!("live-question-popover-{id}")), cx)
                .anchor(Anchor::BottomRight)
                .bottom_2()
                .open(self.questions.open)
                .on_open_change(cx.listener(move |view, open: &bool, _, cx| {
                    if view
                        .questions
                        .editing
                        .as_ref()
                        .is_none_or(|editing| editing.id != id)
                    {
                        return;
                    }
                    view.questions.open = *open;
                    cx.notify();
                }))
                .trigger(
                    Button::new("live-question-trigger")
                        .outline()
                        .small()
                        .rounded_full()
                        .icon(IconName::CircleUser)
                        .font_normal()
                        .text_color(cx.theme().muted_foreground)
                        .label(tr("question_reply"))
                        .debug_selector(|| "live-question-trigger".into()),
                )
                .content(move |_, window, cx| {
                    owner
                        .update(cx, |view, cx| view.question_body(id, window, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .into_any_element(),
        )
    }

    fn question_body(&self, id: QuestionId, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(editing) = self
            .questions
            .editing
            .as_ref()
            .filter(|editing| editing.id == id)
        else {
            return div().into_any_element();
        };
        let locked = self.questions.locked();
        let pending = self.question_pending(id).is_some();
        let closed = !pending && !locked;
        let editable = pending && !locked;
        let plan = matches!(editing.spec.input, Input::Plan);
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .h_auto()
            .min_h_0()
            .flex_shrink_1()
            .gap_4()
            .id(format!("question-scroll-{id}"))
            .overflow_y_scroll()
            .track_scroll(&editing.scroll)
            .vertical_scrollbar(&editing.scroll)
            .child(
                div()
                    .debug_selector(|| "live-question-prompt".into())
                    .w_full()
                    .min_w_0()
                    .text_sm()
                    .font_medium()
                    .whitespace_normal()
                    .child(if plan {
                        tr("plan_execute_confirm").to_string()
                    } else {
                        editing.spec.prompt.clone()
                    }),
            );
        if let Input::Choice {
            options,
            multiple,
            allow_other,
        } = &editing.spec.input
        {
            let labels: Vec<SharedString> = options
                .iter()
                .cloned()
                .map(Into::into)
                .chain(allow_other.then(|| tr("question_other")))
                .collect();
            if *multiple {
                content = content.child(v_flex().w_full().min_w_0().gap_3().children(
                    labels.into_iter().enumerate().map(|(index, label)| {
                        let selected = if index == options.len() {
                            editing.other
                        } else {
                            editing.draft.selected.contains(&index)
                        };

                        Checkbox::new(format!("question-{id}-choice-{index}"))
                            .debug_selector(move || format!("live-question-choice-{index}"))
                            .small()
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .label(label)
                            .checked(selected)
                            .disabled(!editable)
                            .on_click(cx.listener(move |view, selected, window, cx| {
                                view.select_question(id, index, *selected, window, cx)
                            }))
                    }),
                ));
            } else {
                let selected = if editing.other {
                    Some(options.len())
                } else {
                    editing.draft.selected.first().copied()
                };
                content = content.child(
                    RadioGroup::vertical(format!("question-{id}-choices"))
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .selected_index(selected)
                        .disabled(!editable)
                        .children(labels.into_iter().enumerate().map(|(index, label)| {
                            Radio::new(index)
                                .small()
                                .w_full()
                                .min_w_0()
                                .pr(RADIO_INSET)
                                .whitespace_normal()
                                .accessibility_label(label.clone())
                                .child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .whitespace_normal()
                                        .when(!editable, |label| {
                                            label.text_color(cx.theme().muted_foreground)
                                        })
                                        .child(label),
                                )
                                .debug_selector(move || format!("live-question-choice-{index}"))
                        }))
                        .on_click(cx.listener(move |view, index, window, cx| {
                            view.select_question(id, *index, true, window, cx)
                        })),
                );
            }
        }
        if matches!(editing.spec.input, Input::Form { .. }) {
            content = content.child(self.form_fields(editing, editable, cx));
        }
        if let Input::Url { url, .. } = &editing.spec.input {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(url.clone()),
            );
        }
        if let Some(field) = &editing.draft.field {
            let inactive_other =
                matches!(editing.spec.input, Input::Choice { .. }) && !editing.other;
            content = content.child(
                v_flex()
                    .w_full()
                    .debug_selector(|| "live-question-input".into())
                    .child(match field {
                        Field::Line(input) => TextInput::new(input)
                            .aria_label(tr("interaction_input"))
                            .disabled(inactive_other && editable)
                            .readonly(!editable)
                            .into_any_element(),
                        Field::Multiline(input) => Textarea::new(input)
                            .aria_label(tr("interaction_input"))
                            .readonly(!editable)
                            .into_any_element(),
                    }),
            );
        }
        let actions = h_flex()
            .w_full()
            .flex_wrap()
            .gap_2()
            .justify_end()
            .flex_shrink_0()
            .when(
                matches!(editing.spec.input, Input::Form { .. } | Input::Url { .. }) && !closed,
                |row| {
                    row.child(
                        Button::new(format!("question-{id}-decline"))
                            .label(tr("question_decline"))
                            .debug_selector(|| "live-question-decline".into())
                            .disabled(locked || !pending || !self.connected())
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.resolve_question(id, Response::Decline, window, cx)
                            })),
                    )
                },
            )
            .child(
                Button::new(format!("question-{id}-secondary"))
                    .debug_selector(|| "live-question-secondary".into())
                    .label(tr(if closed {
                        "close"
                    } else if plan {
                        "interaction_continue"
                    } else {
                        "interaction_cancel"
                    }))
                    .disabled(locked || !closed && !self.connected())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if closed {
                            view.dismiss_question(id, window, cx);
                        } else if plan {
                            view.resolve_question(
                                id,
                                Response::Answer(Answer::Text(String::new())),
                                window,
                                cx,
                            );
                        } else {
                            view.resolve_question(id, Response::Cancel, window, cx);
                        }
                    })),
            )
            .when(!closed, |row| {
                row.child(
                    Button::new(format!("question-{id}-submit"))
                        .primary()
                        .debug_selector(|| "live-question-submit".into())
                        .label(tr(submit_key(
                            &editing.spec.input,
                            self.questions.pending,
                            self.questions.attempt.is_some(),
                            self.questions.error.is_some(),
                        )))
                        .disabled(
                            self.questions.pending
                                || !self.connected()
                                || self.questions.attempt.is_none()
                                    && (!pending
                                        || if plan {
                                            self.pending || self.retry.is_some()
                                        } else {
                                            editing.answer(cx).is_none()
                                        }),
                        )
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if view
                                .questions
                                .editing
                                .as_ref()
                                .is_none_or(|editing| editing.id != id)
                            {
                                return;
                            }
                            if view.questions.attempt.is_some() {
                                view.retry_question(window, cx);
                                view.focus(window, cx);
                            } else if plan {
                                view.start_coding(id, window, cx);
                            } else {
                                view.answer_question(id, window, cx);
                            }
                        })),
                )
            });
        // Kit clamps the anchored surface into the viewport. Reserve its
        // existing padding and edge margin before the body shares this height.
        let height =
            (window.viewport_size().height - window.rem_size() * 1.5 - px(16.)).max(px(0.));
        v_flex()
            .key_context(self.questions.keys.read(cx).context().as_str())
            .w_80()
            .max_w_full()
            .min_w_0()
            .h_auto()
            .min_h_0()
            .max_h(height)
            .gap_4()
            .debug_selector(|| "pending-question".into())
            .child(content)
            .child(actions)
            .into_any_element()
    }
}

fn submit_key(input: &Input, pending: bool, attempt: bool, error: bool) -> &'static str {
    if !pending && attempt && error {
        "chat_retry"
    } else if matches!(input, Input::Plan) {
        "interaction_start"
    } else if matches!(input, Input::Url { .. }) {
        "question_open_url"
    } else {
        "interaction_submit"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn labels_only_failed_attempts_as_retries() {
        for (input, expected) in [
            (
                Input::Choice {
                    options: vec!["Continue".into(), "Revise".into()],
                    multiple: false,
                    allow_other: false,
                },
                "interaction_submit",
            ),
            (Input::Plan, "interaction_start"),
            (
                Input::Url {
                    url: "https://fixture.invalid/authorize".into(),
                    elicitation_id: "fixture".into(),
                },
                "question_open_url",
            ),
        ] {
            assert_eq!(submit_key(&input, false, false, false), expected);
            assert_eq!(submit_key(&input, true, true, false), expected);
            assert_eq!(submit_key(&input, false, true, false), expected);
            assert_eq!(submit_key(&input, false, true, true), "chat_retry");
            assert_eq!(submit_key(&input, true, true, true), expected);
            assert_eq!(submit_key(&input, false, false, true), expected);
        }
    }
}
