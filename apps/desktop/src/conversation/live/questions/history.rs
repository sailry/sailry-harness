use super::*;
use crate::conversation::{disclosure, surface};
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::scroll::ScrollableElement as _;
use sailry_client::conversation::tools::{Call, State as CallState};
use sailry_protocol::conversation::Page;

impl View {
    pub(in crate::conversation::live) fn question_history(
        &self,
        call: &Call,
        page: &Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let question = call.question.as_ref().expect("question block metadata");
        let spec = call
            .question_spec(page)
            .expect("question block specification");
        let plan = matches!(spec.input, Input::Plan);
        let label = match &question.state {
            State::Answered(Answer::Opened) => "question_url_accepted",
            State::Pending if matches!(spec.input, Input::Plan) => "plan_pending",
            State::Answered(Answer::Plan { .. }) => "plan_accepted",
            State::Pending => "question_pending",
            State::Answered(_) if call.state == CallState::Returned => "question_answered",
            State::Answered(_) if matches!(call.state, CallState::Waiting | CallState::Running) => {
                "question_accepted"
            }
            State::Answered(_) => "question_unconsumed",
            State::Cancelled => "interaction_cancelled",
            State::Declined => "question_declined",
            State::Interrupted => "question_interrupted",
        };
        let answers: Vec<String> = match (&spec.input, &question.state) {
            (Input::Plan, _) => vec![spec.prompt.clone()],
            (Input::Form { fields }, State::Answered(Answer::Form(values))) => fields
                .iter()
                .filter_map(|field| {
                    values
                        .get(&field.name)
                        .map(|value| format!("{}: {}", field.title, fields::display(field, value)))
                })
                .collect(),
            (_, State::Answered(Answer::Text(text))) => vec![text.clone()],
            (
                Input::Choice { options, .. },
                State::Answered(Answer::Choices { selected, other }),
            ) => selected
                .iter()
                .filter_map(|index| options.get(*index).cloned())
                .chain(other.iter().cloned())
                .collect(),
            _ => Vec::new(),
        };
        let id = question.id;
        let turn = call.turn;
        let key = format!("question-{id}");
        let selector = format!("live-question-history-{id}");
        let icon = match question.state {
            State::Pending => IconName::Bot,
            State::Cancelled => IconName::CircleX,
            State::Interrupted => IconName::TriangleAlert,
            _ => IconName::CircleCheck,
        };
        let detail = disclosure::Detail {
            state: (!plan && !matches!(label, "question_answered" | "interaction_cancelled"))
                .then(|| tr(label)),
            wrap: !plan,
            ..Default::default()
        };
        let title = if plan {
            tr("composer_mode_plan")
        } else {
            spec.prompt.into()
        };
        if answers.iter().all(String::is_empty) {
            return disclosure::summary(selector, icon, title, detail, cx).into_any_element();
        }
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(plan);
        let body = if plan {
            surface::result(
                &format!("question-{id}"),
                surface::scroll(
                    format!("question-answer-scroll-{id}"),
                    div()
                        .debug_selector(move || format!("live-question-answer-{id}"))
                        .px_3()
                        .py_2()
                        .text_sm()
                        .child(self.markdown(
                            format!("question-plan-{id}"),
                            answers[0].clone(),
                            cx,
                        )),
                ),
                [],
                cx,
            )
            .into_any_element()
        } else {
            // The answer owns one frame; the question header still spans the row.
            div()
                .debug_selector(move || format!("live-question-answer-frame-{id}"))
                .self_start()
                .max_w_full()
                .min_w_0()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .text_color(cx.theme().muted_foreground)
                .p_3()
                .child(
                    v_flex()
                        .debug_selector(move || format!("live-question-answer-{id}"))
                        .max_w_full()
                        .min_w_0()
                        .max_h(px(surface::OUTPUT_LINES as f32 * surface::LINE_HEIGHT))
                        .gap_1()
                        .text_sm()
                        .children(answers.into_iter().enumerate().map(|(index, answer)| {
                            gpui_kit::base::SelectableText::new(
                                format!("question-answer-{id}-{index}"),
                                answer,
                            )
                        }))
                        .overflow_y_scrollbar()
                        .id(SharedString::from(format!("question-answer-scroll-{id}"))),
                )
                .into_any_element()
        };
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                disclosure::trigger(selector, icon, title, detail, open, cx).on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
            )
            .content(body)
            .into_any_element()
    }
}
