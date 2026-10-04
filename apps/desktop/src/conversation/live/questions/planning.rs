use super::*;
use sailry_protocol::{WorkMode, conversation::question::AcceptedPlan};

impl View {
    pub(super) fn start_coding(
        &mut self,
        id: QuestionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.retry.is_some() || self.readonly() {
            return;
        }
        let Some(session) = &self.session else { return };
        if self
            .questions
            .editing
            .as_ref()
            .is_none_or(|editing| editing.id != id || !matches!(editing.spec.input, Input::Plan))
        {
            return;
        }
        self.resolve_question(
            id,
            Response::StartCoding {
                expected_revision: session.revision,
                message: tr("plan_coding_message").to_string().into(),
            },
            window,
            cx,
        );
    }
}

pub(super) fn valid(request: &Request, accepted: &AcceptedPlan) -> bool {
    let Command::ResolveQuestion {
        session,
        question,
        response: Response::StartCoding {
            expected_revision, ..
        },
    } = &request.command
    else {
        return false;
    };
    accepted.question.id == *question
        && accepted.question.session == *session
        && accepted.question.turn != accepted.turn.id
        && accepted.question.state
            == State::Answered(Answer::Plan {
                turn: accepted.turn.id,
            })
        && accepted.session.id == *session
        && (accepted.session.revision == *expected_revision
            || Some(accepted.session.revision) == expected_revision.checked_add(1))
        && accepted.session.config.mode == WorkMode::Code
        && accepted.turn.session == *session
        && accepted.turn.request == request.id
        && accepted.turn.revision == accepted.session.revision
        && accepted.turn.config == accepted.session.config
        && accepted.turn.roles == accepted.session.roles
}
