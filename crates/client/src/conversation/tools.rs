//! Call/result association shared by every controller. Output remains in canonical history.
use sailry_protocol::{
    TurnId,
    conversation::{Page, Part, Status},
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
pub mod display;

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
    pub presentation: sailry_protocol::tool::Presentation,
    pub grouping: sailry_protocol::tool::Grouping,
    pub source: Reference,
    pub response: Option<Reference>,
    pub state: State,
    pub approval: Option<sailry_protocol::conversation::Approval>,
    pub question: Option<sailry_protocol::conversation::question::Question>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<sailry_protocol::conversation::progress::Progress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<sailry_protocol::tool::Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<display::Resolved>,
}

impl Call {
    pub fn display<'a>(&self, page: &'a Page) -> Option<&'a sailry_protocol::tool::Display> {
        match self.source.part(page)? {
            Part::ToolCall { display, .. } => display.as_deref().filter(|display| display.valid()),
            _ => None,
        }
    }
    pub fn question_spec(
        &self,
        page: &Page,
    ) -> Option<sailry_protocol::conversation::question::Spec> {
        self.question.as_ref()?;
        let spec: sailry_protocol::conversation::question::Spec =
            serde_json::from_value(self.arguments(page)?.clone()).ok()?;
        spec.validate().ok()?;
        Some(spec)
    }
    pub fn arguments<'a>(&self, page: &'a Page) -> Option<&'a Value> {
        match self.source.part(page)? {
            Part::ToolCall { arguments, .. } => Some(arguments),
            _ => None,
        }
    }

    pub fn result<'a>(&self, page: &'a Page) -> Option<&'a Value> {
        match self.response.as_ref()?.part(page)? {
            Part::ToolResult { result, .. } => Some(result),
            _ => None,
        }
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
                        presentation: *presentation,
                        grouping: *grouping,
                        source,
                        response: None,
                        state: State::Waiting,
                        progress: None,
                        content: None,
                        resolved: None,
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
                            presentation: Default::default(),
                            grouping: Default::default(),
                            source: source.clone(),
                            response: Some(source),
                            state: State::Returned,
                            approval: None,
                            question: None,
                            progress: None,
                            content: None,
                            resolved: None,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    for call in &mut calls {
        if call.presentation == sailry_protocol::tool::Presentation::Progress {
            call.grouping = sailry_protocol::tool::Grouping::Standalone;
        }
        call.progress = progress(call, page);
        call.content = content(call, page);
        call.resolved = display::resolve(call, page);
        if matches!(call.state, State::Waiting | State::Running) {
            call.state = match page
                .runs
                .iter()
                .find(|run| run.turn == call.turn)
                .map(|run| run.status)
            {
                Some(Status::Running | Status::Stopping | Status::Queued) => call.state,
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

fn content(call: &Call, page: &Page) -> Option<sailry_protocol::tool::Content> {
    if call.state != State::Returned {
        return None;
    }
    let content = call.presentation.content(call.result(page)?)?;
    let images = call.images(page);
    content
        .blocks
        .iter()
        .all(|block| match block {
            sailry_protocol::tool::Block::Image { index } => {
                images.iter().any(|image| image.index == *index)
            }
            _ => true,
        })
        .then_some(content)
}

fn progress(call: &Call, page: &Page) -> Option<sailry_protocol::conversation::progress::Progress> {
    use sailry_protocol::conversation::progress::Progress;
    if call.presentation != sailry_protocol::tool::Presentation::Progress
        || call.state != State::Returned
    {
        return None;
    }
    let arguments: Progress = serde_json::from_value(call.arguments(page)?.clone()).ok()?;
    arguments.validate().ok()?;
    let result = call.result(page)?.as_object()?;
    if result.len() != 1 {
        return None;
    }
    let progress: Progress = serde_json::from_value(result.get("progress")?.clone()).ok()?;
    (progress == arguments).then_some(progress)
}

#[cfg(test)]
mod tests;
