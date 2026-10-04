//! Only the input draft is UI-owned; question state comes from shared Client history.
use super::*;
use crate::conversation::interaction::{Draft, Field, Kind};
use sailry_protocol::{
    QuestionId,
    conversation::question::{Answer, Input, Question, Response, Spec, State},
};

mod actions;
mod fields;
mod history;
mod planning;
#[cfg(test)]
mod tests;
mod view;

#[derive(Default)]
pub(super) struct Form {
    open: bool,
    editing: Option<Editing>,
    pending: bool,
    attempt: Option<Request>,
    pub(super) error: Option<&'static str>,
    task: Option<Task<()>>,
}

struct Editing {
    id: QuestionId,
    spec: Spec,
    draft: Draft,
    fields: Vec<fields::Draft>,
    other: bool,
    _input: Option<Subscription>,
}

impl Editing {
    fn text(&self, cx: &App) -> String {
        self.draft
            .field
            .as_ref()
            .map(|field| field.value(cx))
            .unwrap_or_default()
    }

    fn answer(&self, cx: &App) -> Option<Answer> {
        let answer = match self.spec.input {
            Input::Url { .. } => Answer::Opened,
            Input::Form { ref fields } => Answer::Form(fields::answer(fields, &self.fields, cx)?),
            Input::Plan => return None,
            Input::Text { .. } => Answer::Text(self.text(cx)),
            Input::Choice { .. } => Answer::Choices {
                selected: self.draft.selected.iter().copied().collect(),
                other: self.other.then(|| self.text(cx)),
            },
        };
        self.spec.validate_answer(&answer).ok()?;
        Some(answer)
    }

    fn empty(&self, cx: &App) -> bool {
        self.draft.selected.is_empty()
            && !self.other
            && self.text(cx).is_empty()
            && self.fields.iter().all(|field| field.empty(cx))
    }
}

impl Form {
    fn locked(&self) -> bool {
        self.pending || self.attempt.is_some()
    }
}

impl View {
    fn question_pending(&self, id: QuestionId) -> Option<&Question> {
        let page = &self.history.snapshot.as_ref()?.page;
        page.questions.iter().find(|question| {
            question.id == id
                && question.state == State::Pending
                && self.session() == Some(question.session)
                && page
                    .runs
                    .iter()
                    .any(|run| run.turn == question.turn && run.status == Status::Running)
        })
    }

    pub(super) fn sync_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.questions.editing.as_ref().is_some_and(|editing| {
            self.question_pending(editing.id).is_none()
                && !self.questions.locked()
                && editing.empty(cx)
        }) {
            self.questions.editing = None;
            self.questions.error = None;
        }
        if self.questions.editing.is_some() {
            return;
        }
        let Some((id, spec)) = self.history.calls.iter().find_map(|call| {
            let question = call.question.as_ref()?;
            self.question_pending(question.id)?;
            Some((
                question.id,
                call.question_spec(&self.history.snapshot.as_ref()?.page)?,
            ))
        }) else {
            return;
        };
        let kind = match &spec.input {
            Input::Form { .. } | Input::Url { .. } => Kind::Plan,
            Input::Plan => Kind::Plan,
            Input::Text {
                multiline,
                max_bytes,
            } => Kind::Text {
                multiline: *multiline,
                max_bytes: *max_bytes,
            },
            Input::Choice {
                options,
                multiple,
                allow_other,
            } => Kind::Choice {
                options: options.iter().cloned().map(Into::into).collect(),
                multiple: *multiple,
                allow_other: *allow_other,
            },
        };
        let draft = Draft::new(&kind, window, cx);
        let fields = match &spec.input {
            Input::Form { fields } => fields
                .iter()
                .map(|field| fields::Draft::new(field, window, cx))
                .collect(),
            _ => Vec::new(),
        };
        let input = match &draft.field {
            Some(Field::Line(input)) => {
                Some(
                    cx.subscribe_in(input, window, move |view, _, event, window, cx| {
                        if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                            view.answer_question(id, window, cx);
                        }
                    }),
                )
            }
            _ => None,
        };
        self.questions.open = true;
        self.questions.editing = Some(Editing {
            id,
            spec,
            draft,
            fields,
            other: false,
            _input: input,
        });
    }

    fn select_question(
        &mut self,
        id: QuestionId,
        index: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if self.questions.locked() || self.question_pending(id).is_none() {
            return;
        }
        let Some(editing) = self
            .questions
            .editing
            .as_mut()
            .filter(|editing| editing.id == id)
        else {
            return;
        };
        let Input::Choice {
            options,
            multiple,
            allow_other,
        } = &editing.spec.input
        else {
            return;
        };
        if index > options.len() || index == options.len() && !allow_other {
            return;
        }
        if selected && !multiple {
            editing.draft.selected.clear();
            editing.other = false;
        }
        if index == options.len() {
            editing.other = selected;
        } else if selected {
            editing.draft.selected.insert(index);
        } else {
            editing.draft.selected.remove(&index);
        }
        cx.notify();
    }

    fn dismiss_question(&mut self, id: QuestionId, window: &mut Window, cx: &mut Context<Self>) {
        if self.questions.locked()
            || self.question_pending(id).is_some()
            || self
                .questions
                .editing
                .as_ref()
                .is_none_or(|editing| editing.id != id)
        {
            return;
        }
        self.questions.editing = None;
        self.questions.error = None;
        self.focus(window, cx);
        cx.notify();
    }
}
