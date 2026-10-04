//! Question and plan presentation only; no request is sent to an execution node.

mod view;

use std::collections::BTreeSet;

use super::turn::{Block, Status, Target};
use crate::{shell::Shell, tr};
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Choice {
        options: Vec<SharedString>,
        multiple: bool,
        allow_other: bool,
    },
    Text {
        multiline: bool,
        max_bytes: usize,
    },
    Secret,
    Plan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Pending,
    Answered(Vec<SharedString>),
    ContinuePlanning,
    StartCoding,
    Cancelled,
}

impl State {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "interaction_pending",
            Self::Answered(_) => "interaction_answered",
            Self::ContinuePlanning => "interaction_continue_recorded",
            Self::StartCoding => "interaction_start_recorded",
            Self::Cancelled => "interaction_cancelled",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub id: usize,
    pub generation: usize,
    pub prompt: SharedString,
    pub kind: Kind,
    pub state: State,
}

pub(crate) enum Field {
    Line(Entity<InputState>),
    Multiline(Entity<TextareaState>),
}

impl Field {
    pub(crate) fn value(&self, cx: &App) -> String {
        match self {
            Self::Line(input) => input.read(cx).value().to_string(),
            Self::Multiline(input) => input.read(cx).value().to_string(),
        }
    }
}

pub(crate) struct Draft {
    pub selected: BTreeSet<usize>,
    pub field: Option<Field>,
}

impl Draft {
    pub fn new<T: 'static>(kind: &Kind, window: &mut Window, cx: &mut Context<T>) -> Self {
        let field = match kind {
            Kind::Text {
                multiline: true, ..
            } => {
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .placeholder(tr("interaction_input"))
                        .auto_grow(2, 3)
                });
                cx.observe(&input, |_, _, cx| cx.notify()).detach();
                Some(Field::Multiline(input))
            }
            Kind::Text { .. }
            | Kind::Choice {
                allow_other: true, ..
            } => {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder(tr("interaction_input")));
                cx.observe(&input, |_, _, cx| cx.notify()).detach();
                Some(Field::Line(input))
            }
            _ => None,
        };
        Self {
            selected: BTreeSet::new(),
            field,
        }
    }

    pub fn answer(&self, kind: &Kind, cx: &App) -> Option<Vec<SharedString>> {
        let text = self
            .field
            .as_ref()
            .map(|field| field.value(cx))
            .unwrap_or_default();
        let text = text.trim();
        match kind {
            Kind::Choice {
                options,
                multiple,
                allow_other,
            } => {
                if !multiple && self.selected.len() > 1 {
                    return None;
                }
                let mut answers = self
                    .selected
                    .iter()
                    .map(|&index| options.get(index).cloned())
                    .collect::<Option<Vec<_>>>()?;
                if *allow_other && !text.is_empty() {
                    answers.push(text.to_owned().into());
                }
                (!answers.is_empty()).then_some(answers)
            }
            Kind::Text { max_bytes, .. } if !text.is_empty() && text.len() <= *max_bytes => {
                Some(vec![text.to_owned().into()])
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Response {
    Submit,
    Continue,
    Start,
    Cancel,
}

impl Shell {
    pub(crate) fn select_answer(
        &mut self,
        target: Target,
        index: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.pending_request(target) else {
            return;
        };
        let Kind::Choice {
            options, multiple, ..
        } = &request.kind
        else {
            return;
        };
        if index >= options.len() {
            return;
        }
        let multiple = *multiple;
        let Some(draft) = self
            .conversations
            .get_mut(&target.session)
            .and_then(|thread| thread.interaction_drafts.get_mut(&target))
        else {
            return;
        };
        if selected {
            if !multiple {
                draft.selected.clear();
            }
            draft.selected.insert(index);
        } else {
            draft.selected.remove(&index);
        }
        cx.notify();
    }

    fn pending_request(&self, target: Target) -> Option<&Request> {
        let turn = self
            .conversations
            .get(&target.session)?
            .turns
            .get(target.turn)?;
        if turn.status != Status::Waiting {
            return None;
        }
        turn.blocks.iter().find_map(|block| match block {
            Block::Interaction(request)
                if request.id == target.request
                    && request.generation == target.generation
                    && request.state == State::Pending =>
            {
                Some(request)
            }
            _ => None,
        })
    }

    pub(crate) fn resolve_interaction(
        &mut self,
        target: Target,
        response: Response,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.pending_request(target) else {
            return;
        };
        let state = match (&request.kind, response) {
            (Kind::Plan, Response::Continue) => State::ContinuePlanning,
            (Kind::Plan, Response::Start) => State::StartCoding,
            (_, Response::Cancel) => State::Cancelled,
            (_, Response::Submit) => {
                let Some(answer) = self.conversations[&target.session]
                    .interaction_drafts
                    .get(&target)
                    .and_then(|draft| draft.answer(&request.kind, cx))
                else {
                    return;
                };
                State::Answered(answer)
            }
            _ => return,
        };
        let thread = self.conversations.get_mut(&target.session).unwrap();
        let turn = &mut thread.turns[target.turn];
        for block in &mut turn.blocks {
            if let Block::Interaction(request) = block
                && request.id == target.request
                && request.generation == target.generation
            {
                request.state = state.clone();
                break;
            }
        }
        thread.interaction_drafts.remove(&target);
        if !turn.has_pending_requests() {
            turn.status = Status::Completed;
            turn.finished_at = Some(tr("turn_preview_time"));
            turn.blocks
                .push(Block::Text(tr("interaction_preview_result")));
        }
        thread.scroller.update(cx, |state, cx| {
            state.remeasure_items(target.turn..target.turn + 1, cx)
        });
        cx.notify();
    }
}
