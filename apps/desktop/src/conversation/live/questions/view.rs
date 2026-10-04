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
        let locked = self.questions.locked();
        let pending = self.question_pending(id).is_some();
        let closed = !pending && !locked;
        let editable = pending && !locked;
        let plan = matches!(editing.spec.input, Input::Plan);
        let mut content = v_flex()
            .w_full()
            .h_auto()
            .flex_shrink_0()
            .max_h_64()
            .gap_4()
            .overflow_y_scrollbar()
            .id(format!("question-scroll-{id}"))
            .child(
                div()
                    .debug_selector(|| "live-question-prompt".into())
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
                content = content.child(v_flex().w_full().gap_3().children(
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
                            .label(label)
                            .checked(selected)
                            .disabled(!editable)
                            .on_click(cx.listener(move |view, selected, _, cx| {
                                view.select_question(id, index, *selected, cx)
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
                        .gap_3()
                        .selected_index(selected)
                        .disabled(!editable)
                        .children(labels.into_iter().enumerate().map(|(index, label)| {
                            Radio::new(index)
                                .small()
                                .w_full()
                                .label(label)
                                .debug_selector(move || format!("live-question-choice-{index}"))
                        }))
                        .on_click(cx.listener(move |view, index, _, cx| {
                            view.select_question(id, *index, true, cx)
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
                        .label(tr(if self.questions.attempt.is_some() {
                            "chat_retry"
                        } else if plan {
                            "interaction_start"
                        } else if matches!(editing.spec.input, Input::Url { .. }) {
                            "question_open_url"
                        } else {
                            "interaction_submit"
                        }))
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
        let body = v_flex()
            .w_80()
            .max_w_full()
            .gap_4()
            .debug_selector(|| "pending-question".into())
            .child(content)
            .child(actions);
        Some(
            Popover::new(format!("live-question-popover-{id}"))
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
                .child(body)
                .into_any_element(),
        )
    }
}
