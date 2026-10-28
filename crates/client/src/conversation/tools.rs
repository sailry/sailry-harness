//! Call/result association shared by every controller. Output remains in canonical history.
use sailry_protocol::{
    TurnId,
    conversation::{Page, Part, Status},
};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
mod approval;
mod changes;
pub mod display;
mod presentation;
mod row;
pub use approval::{ApprovalView, decision_ready};
pub use changes::Stamp;
pub use row::{Row, RowState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Waiting,
    Running,
    Returned,
    Cancelled,
    NotExecuted,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub entry: String,
    pub index: usize,
}

impl Reference {
    pub fn key(&self) -> String {
        format!("{}-{}", self.entry, self.index)
    }

    fn part<'a>(&self, page: &'a Page) -> Option<&'a Part> {
        page.entries
            .iter()
            .find(|entry| entry.id == self.entry)?
            .parts
            .get(self.index)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Call {
    pub turn: TurnId,
    pub id: Option<String>,
    pub name: String,
    pub terminal_interaction: bool,
    pub presentation: sailry_protocol::tool::Presentation,
    pub grouping: sailry_protocol::tool::Grouping,
    pub source: Reference,
    pub response: Option<Reference>,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<sailry_protocol::tool::Failure>,
    pub approval: Option<sailry_protocol::conversation::Approval>,
    pub question: Option<sailry_protocol::conversation::question::Question>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<sailry_protocol::conversation::progress::Progress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<display::Resolved>,
    #[serde(skip)]
    pub detail: Option<Arc<sailry_protocol::tool::detail::Detail>>,
}

impl Call {
    pub fn children(&self, page: &Page) -> Vec<super::children::Reference> {
        super::children::associated(page, self.turn, &self.source.entry, self.source.index)
    }

    /// Completed synthetic plan review is consent, not a running tool request.
    pub fn question_pending<'a>(
        &self,
        page: &'a Page,
    ) -> Option<&'a sailry_protocol::conversation::question::Question> {
        if self.response.is_some() {
            return None;
        }
        use sailry_protocol::conversation::{RunKind, question::State};
        let question = page.questions.iter().find(|question| {
            self.question
                .as_ref()
                .is_some_and(|owned| owned.id == question.id)
                && question.turn == self.turn
                && question.entry == self.source.entry
                && question.index == self.source.index
                && question.session == page.session
                && question.state == State::Pending
        })?;
        let run = page.runs.iter().find(|run| run.turn == question.turn)?;
        if run.session != page.session || run.origin.is_some() || run.kind != RunKind::Task {
            return None;
        }
        if run.status == Status::Running {
            return Some(question);
        }
        if self.completion_review(page) {
            Some(question)
        } else {
            None
        }
    }

    fn completion_review(&self, page: &Page) -> bool {
        use sailry_protocol::conversation::{
            RunKind,
            question::{Input, State},
        };
        self.name == "plan_review"
            && self.response.is_none()
            && self
                .question
                .as_ref()
                .is_some_and(|question| question.state == State::Pending)
            && page.runs.iter().any(|run| {
                run.turn == self.turn
                    && run.kind == RunKind::Task
                    && run.status == Status::Completed
                    && run.error.is_none()
            })
            && page
                .entries
                .iter()
                .any(|entry| entry.id == self.source.entry && entry.author == "plan_review")
            && self
                .question_spec(page)
                .is_some_and(|spec| spec.prompt.is_empty() && matches!(spec.input, Input::Plan))
    }

    pub fn summary<'a>(
        &self,
        page: &'a Page,
    ) -> Option<&'a sailry_protocol::tool::detail::Summary> {
        let reference = self.response.as_ref().unwrap_or(&self.source);
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.id == reference.entry && entry.turn == self.turn)?;
        match entry.parts.get(reference.index)? {
            Part::ToolCall {
                id, name, summary, ..
            }
            | Part::ToolResult {
                id, name, summary, ..
            } if *id == self.id && *name == self.name => Some(summary),
            _ => None,
        }
    }
    pub fn detail_reference(
        &self,
        page: &Page,
    ) -> Option<sailry_protocol::tool::detail::Reference> {
        let source = self.response.as_ref().unwrap_or(&self.source);
        Some(sailry_protocol::tool::detail::Reference {
            session: page.session,
            turn: self.turn,
            entry: source.entry.clone(),
            index: source.index,
            call: self.id.clone(),
            revision: self.summary(page)?.revision.clone(),
        })
    }
    pub fn with_detail(&self, detail: Arc<sailry_protocol::tool::detail::Detail>) -> Self {
        let mut call = self.clone();
        call.detail = Some(detail);
        call
    }
    pub fn resolved(&self) -> Option<&display::Resolved> {
        self.detail
            .as_ref()
            .and_then(|detail| detail.resolved.as_ref())
            .or(self.resolved.as_ref())
    }
    pub fn content(&self) -> Option<&sailry_protocol::tool::Content> {
        self.detail
            .as_ref()
            .and_then(|detail| detail.content.as_ref())
    }
    pub fn question_spec(
        &self,
        page: &Page,
    ) -> Option<sailry_protocol::conversation::question::Spec> {
        self.question.as_ref()?;
        let Part::ToolCall { summary, .. } = self.source.part(page)? else {
            return None;
        };
        let spec = summary.question.as_deref()?.clone();
        spec.validate().ok()?;
        Some(spec)
    }
    pub fn arguments<'a>(&'a self, page: &Page) -> Option<&'a Value> {
        self.summary(page)?;
        self.detail.as_ref().map(|detail| &detail.arguments)
    }

    pub fn result<'a>(&'a self, page: &Page) -> Option<&'a Value> {
        self.response.as_ref()?;
        self.summary(page)?;
        self.detail.as_ref()?.result.as_ref()
    }

    pub fn images<'a>(&self, page: &'a Page) -> &'a [sailry_protocol::conversation::Image] {
        match self.response.as_ref().and_then(|source| source.part(page)) {
            Some(Part::ToolResult { images, .. }) => images,
            _ => &[],
        }
    }
}

pub(super) fn collect(page: &Page) -> Vec<Call> {
    let mut calls: Vec<Call> = Vec::new();
    let mut pending: BTreeMap<(TurnId, &str, &str), Vec<usize>> = BTreeMap::new();
    for entry in &page.entries {
        for (index, part) in entry.parts.iter().enumerate() {
            let source = Reference {
                entry: entry.id.clone(),
                index,
            };
            match part {
                Part::ToolCall {
                    id,
                    name,
                    presentation,
                    grouping,
                    ..
                } => {
                    if let Some(id) = id {
                        pending
                            .entry((entry.turn, &entry.branch, id))
                            .or_default()
                            .push(calls.len());
                    }
                    calls.push(Call {
                        turn: entry.turn,
                        id: id.clone(),
                        name: name.clone(),
                        terminal_interaction: super::reply::terminal_interaction(name),
                        presentation: *presentation,
                        grouping: *grouping,
                        source,
                        response: None,
                        state: State::Waiting,
                        failure: None,
                        progress: None,
                        resolved: None,
                        detail: None,
                        question: page
                            .questions
                            .iter()
                            .find(|question| {
                                question.turn == entry.turn
                                    && question.entry == entry.id
                                    && question.index == index
                            })
                            .cloned(),
                        approval: page
                            .approvals
                            .iter()
                            .find(|approval| {
                                approval.turn == entry.turn
                                    && approval.entry == entry.id
                                    && approval.index == index
                            })
                            .cloned(),
                    });
                }
                Part::Resource(value) if value["type"] == "tool_started" => {
                    if let Some(id) = value["id"].as_str()
                        && let Some(indices) = pending.get(&(entry.turn, entry.branch.as_str(), id))
                        && let [index] = indices.as_slice()
                        && value["name"].as_str() == Some(calls[*index].name.as_str())
                    {
                        calls[*index].state = State::Running;
                    }
                }
                Part::ToolResult { id, name, .. } => {
                    let candidates = id.as_ref().and_then(|id| {
                        pending.remove(&(entry.turn, entry.branch.as_str(), id.as_str()))
                    });
                    if let Some(indices) = candidates
                        && let [index] = indices.as_slice()
                        && calls[*index].name == *name
                    {
                        calls[*index].response = Some(source);
                        calls[*index].state = State::Returned;
                    } else {
                        // Missing, repeated or mismatched identities must not attach a result to another call.
                        calls.push(Call {
                            turn: entry.turn,
                            id: id.clone(),
                            name: name.clone(),
                            terminal_interaction: super::reply::terminal_interaction(name),
                            presentation: Default::default(),
                            grouping: Default::default(),
                            source: source.clone(),
                            response: Some(source),
                            state: State::Returned,
                            failure: None,
                            approval: None,
                            question: None,
                            progress: None,
                            resolved: None,
                            detail: None,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    for call in &mut calls {
        call.failure = call
            .summary(page)
            .and_then(|summary| summary.failure.clone());
        if call.presentation == sailry_protocol::tool::Presentation::Progress {
            call.grouping = sailry_protocol::tool::Grouping::Standalone;
        }
        call.progress = call
            .summary(page)
            .and_then(|summary| summary.progress.clone());
        call.resolved = call
            .summary(page)
            .and_then(|summary| summary.resolved.clone());
        if matches!(call.state, State::Waiting | State::Running) {
            call.state = match page
                .runs
                .iter()
                .find(|run| run.turn == call.turn)
                .map(|run| run.status)
            {
                Some(Status::Running | Status::Stopping | Status::Queued) => call.state,
                Some(Status::Completed) if call.completion_review(page) => State::Waiting,
                Some(Status::Cancelled)
                    if call.approval.as_ref().is_none_or(|approval| {
                        approval.state != sailry_protocol::conversation::ApprovalState::Approved
                    }) && call.question.as_ref().is_none_or(|question| {
                        !matches!(
                            question.state,
                            sailry_protocol::conversation::question::State::Answered(_)
                        )
                    }) =>
                {
                    State::Cancelled
                }
                _ if call.state == State::Waiting
                    && call.approval.as_ref().is_some_and(|approval| {
                        matches!(
                            approval.state,
                            sailry_protocol::conversation::ApprovalState::Interrupted
                                | sailry_protocol::conversation::ApprovalState::Cancelled
                        )
                    }) =>
                {
                    State::NotExecuted
                }
                _ => State::Interrupted,
            };
        }
    }
    calls
}

#[cfg(test)]
mod tests;
