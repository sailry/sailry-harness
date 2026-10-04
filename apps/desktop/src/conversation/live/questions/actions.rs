use super::*;
use sailry_protocol::ErrorCode;

impl View {
    pub(super) fn answer_question(
        &mut self,
        id: QuestionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.questions.locked() || !self.connected() || self.question_pending(id).is_none() {
            return;
        }
        let Some(answer) = self
            .questions
            .editing
            .as_ref()
            .filter(|editing| editing.id == id)
            .and_then(|editing| editing.answer(cx))
        else {
            return;
        };
        if let Some(Editing {
            spec:
                Spec {
                    input: Input::Url { url, .. },
                    ..
                },
            ..
        }) = &self.questions.editing
        {
            cx.open_url(url);
        }
        self.resolve_question(id, Response::Answer(answer), window, cx);
    }

    pub(super) fn resolve_question(
        &mut self,
        id: QuestionId,
        response: Response,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.questions.locked() || !self.connected() {
            return;
        }
        let Some(question) = self.question_pending(id) else {
            return;
        };
        self.questions.attempt = Some(self.binding.client.prepare(Command::ResolveQuestion {
            session: question.session,
            question: id,
            response,
        }));
        self.retry_question(window, cx);
        self.focus(window, cx);
    }

    pub(super) fn retry_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.questions.pending || !self.connected() {
            return;
        }
        let Some(request) = self.questions.attempt.clone() else {
            return;
        };
        let Command::ResolveQuestion {
            session,
            question,
            ref response,
        } = request.command
        else {
            return;
        };
        let expected = match response {
            Response::StartCoding { .. } => None,
            Response::Answer(answer) => Some(State::Answered(answer.clone())),
            Response::Cancel => Some(State::Cancelled),
            Response::Decline => Some(State::Declined),
        };
        self.questions.pending = true;
        self.questions.error = None;
        let client = self.binding.client.clone();
        let original = request.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.questions.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = job.await;
            let _ = view.update_in(cx, |view, _, cx| {
                view.questions.pending = false;
                match result {
                    Ok(Ok(Output::PlanAccepted(accepted)))
                        if planning::valid(&original, &accepted) =>
                    {
                        if view.session.as_ref().is_some_and(|current| {
                            current.id == accepted.session.id
                                && current.revision <= accepted.session.revision
                        }) {
                            view.session = Some(accepted.session);
                            view.refresh_config(cx);
                        }
                        view.finish_question(question);
                    }
                    Ok(Ok(Output::Question(value)))
                        if value.id == question
                            && value.session == session
                            && Some(&value.state) == expected.as_ref() =>
                    {
                        view.finish_question(question);
                    }
                    Ok(Err(error)) => {
                        view.questions.error = Some(match error.code {
                            ErrorCode::InvalidRequest => "question_invalid_answer",
                            ErrorCode::RevisionConflict => "plan_config_changed",
                            ErrorCode::Conflict | ErrorCode::NotFound => "question_closed",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => {
                                "chat_action_unknown"
                            }
                            _ => "chat_action_failed",
                        });
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                        ) {
                            view.questions.attempt = None;
                        }
                    }
                    _ => view.questions.error = Some("chat_action_unknown"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn finish_question(&mut self, question: QuestionId) {
        self.questions.attempt = None;
        if self
            .questions
            .editing
            .as_ref()
            .is_some_and(|editing| editing.id == question)
        {
            self.questions.editing = None;
        }
    }
}
