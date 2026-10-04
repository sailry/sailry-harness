use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::{Input, Textarea},
    radio::Radio,
    scroll::ScrollableElement,
    separator::Separator,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;

impl Shell {
    pub(in crate::conversation) fn pending_interaction(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = (self.host, self.session);
        let thread = &self.conversations[&session];
        let turn = thread.turns.last()?;
        if turn.status != Status::Waiting || turn.blocks.iter().any(|block| {
            matches!(block, Block::Approval(request) if request.state == super::super::approval::State::Pending)
        }) { return None; }
        let request = turn.blocks.iter().find_map(|block| match block {
            Block::Interaction(request) if request.state == State::Pending => Some(request),
            _ => None,
        })?;
        let target = Target {
            session,
            turn: thread.turns.len() - 1,
            request: request.id,
            generation: request.generation,
        };
        let draft = thread.interaction_drafts.get(&target)?;
        let mut content = v_flex()
            .w_full()
            .flex_grow(1.)
            .p_1()
            .gap_2()
            .overflow_y_scrollbar()
            .id(format!(
                "interaction-scroll-{}-{}-{}-{}-{}",
                session.0, session.1, target.turn, target.request, target.generation
            ))
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .whitespace_normal()
                    .child(request.prompt.clone()),
            );
        if let Kind::Choice {
            options, multiple, ..
        } = &request.kind
        {
            content = content.children(options.iter().enumerate().map(|(index, option)| {
                let selected = draft.selected.contains(&index);
                let row = if *multiple {
                    Checkbox::new(("question-choice", index))
                        .small()
                        .w_full()
                        .label(option.clone())
                        .checked(selected)
                        .on_click(cx.listener(move |shell, selected, _, cx| {
                            shell.select_answer(target, index, *selected, cx)
                        }))
                        .into_any_element()
                } else {
                    Radio::new(("question-choice", index))
                        .small()
                        .w_full()
                        .label(option.clone())
                        .checked(selected)
                        .on_click(cx.listener(move |shell, selected, _, cx| {
                            shell.select_answer(target, index, *selected, cx)
                        }))
                        .into_any_element()
                };
                div()
                    .w_full()
                    .debug_selector(move || format!("question-choice-{index}"))
                    .child(row)
            }));
        }
        if let Some(field) = &draft.field {
            content = content.child(
                div()
                    .w_full()
                    .debug_selector(|| "question-input".into())
                    .child(match field {
                        Field::Line(input) => Input::new(input)
                            .small()
                            .aria_label(tr("interaction_input"))
                            .into_any_element(),
                        Field::Multiline(input) => Textarea::new(input)
                            .aria_label(tr("interaction_input"))
                            .into_any_element(),
                    }),
            );
        }
        if let Kind::Text { max_bytes, .. } = request.kind {
            let bytes = draft
                .field
                .as_ref()
                .map(|field| field.value(cx).trim().len())
                .unwrap_or_default();
            content = content.child(
                div()
                    .text_xs()
                    .text_color(if bytes > max_bytes {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(
                        rust_i18n::t!("interaction_byte_limit", used = bytes, limit = max_bytes)
                            .to_string(),
                    ),
            );
        }
        if matches!(request.kind, Kind::Secret) {
            content = content.child(
                div()
                    .text_sm()
                    .whitespace_normal()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("interaction_secret_unavailable")),
            );
        }
        let plan = matches!(request.kind, Kind::Plan);
        let mut actions = h_flex()
            .w_full()
            .gap_2()
            .justify_end()
            .flex_shrink_0()
            .child(
                Button::new("interaction-secondary")
                    .small()
                    .debug_selector(|| "interaction-secondary".into())
                    .label(tr(if plan {
                        "interaction_continue"
                    } else {
                        "interaction_cancel"
                    }))
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.resolve_interaction(
                            target,
                            if plan {
                                Response::Continue
                            } else {
                                Response::Cancel
                            },
                            cx,
                        );
                        shell.resume_queued_preview(target.session, window, cx);
                        if (shell.host, shell.session) == target.session {
                            shell.conversations[&target.session]
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                    })),
            );
        if !matches!(request.kind, Kind::Secret) {
            actions = actions.child(
                Button::new("interaction-submit")
                    .primary()
                    .small()
                    .debug_selector(|| "interaction-submit".into())
                    .label(tr(if plan {
                        "interaction_start"
                    } else {
                        "interaction_submit"
                    }))
                    .disabled(!plan && draft.answer(&request.kind, cx).is_none())
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.resolve_interaction(
                            target,
                            if plan {
                                Response::Start
                            } else {
                                Response::Submit
                            },
                            cx,
                        );
                        shell.resume_queued_preview(target.session, window, cx);
                        if (shell.host, shell.session) == target.session {
                            shell.conversations[&target.session]
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                    })),
            );
        }
        Some(
            v_flex()
                .w_full()
                .max_h_56()
                .gap_2()
                .debug_selector(|| "pending-interaction".into())
                .child(Separator::horizontal())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("interaction_preview_notice")),
                )
                .child(content)
                .child(actions)
                .into_any_element(),
        )
    }

    pub(in crate::conversation) fn interaction_summary(
        &self,
        id: String,
        request: &Request,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .id(id.clone())
            .w_full()
            .gap_2()
            .items_start()
            .debug_selector(move || format!("interaction-history-{id}"))
            .child(
                Icon::new(match request.state {
                    State::Pending => IconName::Bot,
                    State::Cancelled => IconName::CircleX,
                    _ => IconName::CircleCheck,
                })
                .size_4(),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .whitespace_normal()
                            .child(request.prompt.clone()),
                    )
                    .when(request.state != State::Pending, |body| {
                        body.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(request.state.label())),
                        )
                    })
                    .when_some(
                        match &request.state {
                            State::Answered(answers) => Some(answers),
                            _ => None,
                        },
                        |body, answers| {
                            body.children(answers.iter().map(|answer| {
                                div().text_sm().whitespace_normal().child(answer.clone())
                            }))
                        },
                    ),
            )
            .into_any_element()
    }
}
