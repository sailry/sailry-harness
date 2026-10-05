use super::*;
use sailry_protocol::{
    SessionId,
    conversation::{Entry, Run},
};
use serde_json::json;

mod content;
mod display;
mod progress;

fn entry(sequence: u64, turn: TurnId, branch: &str, result: bool) -> Entry {
    Entry {
        sequence,
        id: format!("event-{sequence}"),
        turn,
        author: "assistant".into(),
        branch: branch.into(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        parts: vec![if result {
            Part::ToolResult {
                id: Some("call".into()),
                name: "read_file".into(),
                result: json!({"text":"中文 🙂"}),
                images: vec![],
            }
        } else {
            Part::ToolCall {
                display: None,
                presentation: Default::default(),
                grouping: Default::default(),
                id: Some("call".into()),
                name: "read_file".into(),
                arguments: json!({"path":"source.rs"}),
            }
        }],
    }
}

fn page() -> Page {
    let session = SessionId::new();
    let turn = TurnId::new();
    Page {
        session,
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: Vec::new(),
        questions: Vec::new(),
        children: Vec::new(),
        entries: vec![entry(1, turn, "", false)],
        runs: vec![Run {
            worktree: sailry_protocol::WorktreeId::new(),
            kind: sailry_protocol::conversation::RunKind::Task,
            turn,
            session,
            sequence: 1,
            revision: 1,
            status: Status::Running,
            error: None,
            started_ms: Some(1000),
            finished_ms: None,
            origin: None,
        }],
    }
}

#[test]
fn references_results() {
    let mut page = page();
    assert_eq!(collect(&page)[0].state, State::Waiting);
    page.entries.push(entry(2, page.runs[0].turn, "", true));
    let calls = collect(&page);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].source.key(), "event-1-0");
    assert_eq!(calls[0].state, State::Returned);
    assert_eq!(calls[0].arguments(&page).unwrap()["path"], "source.rs");
    assert_eq!(calls[0].result(&page).unwrap()["text"], "中文 🙂");
}

#[test]
fn retains_localized_display_without_a_live_package() {
    let mut page = page();
    let display = serde_json::from_value(json!({
        "label":"Read", "locales":{"zh-CN":"读取"},
        "input":{"summary":{"path":"/path"}}
    }))
    .unwrap();
    if let Part::ToolCall { display: saved, .. } = &mut page.entries[0].parts[0] {
        *saved = Some(display);
    }
    let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
    let calls = collect(&restored);
    assert_eq!(calls[0].display(&restored).unwrap().label("zh-CN"), "读取");
    assert_eq!(calls[0].display(&restored).unwrap().label("en"), "Read");
    assert_eq!(
        calls[0]
            .display(&restored)
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .read(calls[0].arguments(&restored).unwrap()),
        Some("source.rs")
    );
}

#[test]
fn uses_declared_presentation() {
    let mut page = page();
    if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
        *presentation = sailry_protocol::tool::Presentation::Summary;
    }
    page.entries.push(entry(2, page.runs[0].turn, "", true));
    assert_eq!(
        collect(&page)[0].presentation,
        sailry_protocol::tool::Presentation::Summary
    );
    let rebuilt: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
    assert_eq!(
        collect(&rebuilt)[0].presentation,
        collect(&page)[0].presentation
    );
}

#[test]
fn approval_does_not_imply_execution() {
    let mut page = page();
    let approval = sailry_protocol::conversation::Approval {
        id: sailry_protocol::ApprovalId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: page.entries[0].id.clone(),
        index: 0,
        state: sailry_protocol::conversation::ApprovalState::Approved,
        source: sailry_protocol::conversation::ApprovalSource::User,
    };
    page.approvals.push(approval.clone());
    assert_eq!(collect(&page)[0].approval.as_ref(), Some(&approval));
    assert_eq!(collect(&page)[0].state, State::Waiting);
    page.runs[0].status = Status::Cancelled;
    assert_eq!(collect(&page)[0].state, State::Interrupted);
    page.entries.push(entry(2, page.runs[0].turn, "", true));
    assert_eq!(collect(&page)[0].state, State::Returned);
    page.approvals[0].turn = TurnId::new();
    assert!(collect(&page)[0].approval.is_none());
}

#[test]
fn distinguishes_denial_and_interruption() {
    use sailry_protocol::conversation::{Approval, ApprovalSource, ApprovalState};
    let mut page = page();
    page.runs[0].status = Status::Interrupted;
    page.approvals.push(Approval {
        id: sailry_protocol::ApprovalId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: page.entries[0].id.clone(),
        index: 0,
        state: ApprovalState::Interrupted,
        source: ApprovalSource::User,
    });
    assert_eq!(collect(&page)[0].state, State::NotExecuted);
    page.approvals[0].state = ApprovalState::Approved;
    assert_eq!(collect(&page)[0].state, State::Interrupted);
    page.approvals[0].state = ApprovalState::Interrupted;
    let mut started = entry(2, page.runs[0].turn, "", false);
    started.parts = vec![Part::Resource(json!({
        "type": "tool_started", "id": "call", "name": "read_file"
    }))];
    page.entries.push(started);
    assert_eq!(collect(&page)[0].state, State::Interrupted);
    page.entries.push(entry(3, page.runs[0].turn, "", true));
    assert_eq!(collect(&page)[0].state, State::Returned);
}

#[test]
fn isolates_reused_ids() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.entries.push(entry(2, turn, "child", false));
    page.entries.push(entry(3, turn, "child", true));
    page.entries.push(entry(4, TurnId::new(), "", true));
    let calls = collect(&page);
    assert_eq!(calls.len(), 3);
    assert!(calls[0].result(&page).is_none());
    assert_eq!(calls[1].response.as_ref().unwrap().entry, "event-3");
    assert_eq!(calls[2].source.entry, "event-4");
    page.entries.push(entry(5, turn, "", true));
    assert_eq!(
        collect(&page)[0].response.as_ref().unwrap().entry,
        "event-5"
    );
}

#[test]
fn retains_unconsumed_answers() {
    use sailry_protocol::conversation::question::{Answer, Question, State as QuestionState};
    let mut page = page();
    page.entries[0].parts[0] = Part::ToolCall {
        display: None,
        presentation: Default::default(),
        grouping: Default::default(),
        id: Some("call".into()),
        name: "ask_user".into(),
        arguments: json!({"prompt": "Describe", "input": {"kind": "text", "multiline": true, "max_bytes": 100}}),
    };
    let question = Question {
        id: sailry_protocol::QuestionId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: page.entries[0].id.clone(),
        index: 0,
        state: QuestionState::Answered(Answer::Text("answer".into())),
    };
    page.questions.push(question.clone());
    let calls = collect(&page);
    assert_eq!(calls[0].question, Some(question));
    assert_eq!(calls[0].question_spec(&page).unwrap().prompt, "Describe");
    assert_eq!(calls[0].state, State::Waiting);
    page.runs[0].status = Status::Cancelled;
    assert_eq!(collect(&page)[0].state, State::Interrupted);
    page.questions[0].state = QuestionState::Cancelled;
    assert_eq!(collect(&page)[0].state, State::Cancelled);
    page.questions[0].turn = TurnId::new();
    assert!(collect(&page)[0].question.is_none());
}

#[test]
fn started_question_retains_pending_input() {
    use sailry_protocol::conversation::question::{Question, State as QuestionState};
    let mut page = page();
    page.entries[0].parts[0] = Part::ToolCall {
        display: None,
        presentation: Default::default(),
        grouping: Default::default(),
        id: Some("call".into()),
        name: "ask_user".into(),
        arguments: json!({"prompt":"Describe", "input":{"kind":"text", "multiline":true, "max_bytes":100}}),
    };
    let question = Question {
        id: sailry_protocol::QuestionId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: page.entries[0].id.clone(),
        index: 0,
        state: QuestionState::Pending,
    };
    page.questions.push(question.clone());
    let mut started = entry(2, question.turn, "", false);
    started.parts = vec![Part::Resource(json!({
        "type":"tool_started", "id":"call", "name":"ask_user"
    }))];
    page.entries.push(started);
    let calls = collect(&page);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].state, State::Running);
    assert_eq!(calls[0].question, Some(question.clone()));
    assert_eq!(calls[0].source.entry, question.entry);
    assert_eq!(calls[0].source.index, question.index);
    assert!(calls[0].response.is_none());
    assert_eq!(calls[0].question_spec(&page).unwrap().prompt, "Describe");
}

#[test]
fn retains_uncertain_calls() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.entries.push(entry(2, turn, "", false));
    page.entries.push(entry(3, turn, "", true));
    let calls = collect(&page);
    assert_eq!(calls.len(), 3);
    assert!(calls[..2].iter().all(|call| call.response.is_none()));
    page.runs[0].status = Status::Cancelled;
    assert_eq!(collect(&page)[0].state, State::Cancelled);
    page.runs[0].status = Status::Completed;
    assert_eq!(collect(&page)[0].state, State::Interrupted);
}

#[test]
fn reuses_calls_during_streaming() {
    use super::super::View;
    use sailry_protocol::{
        NodeId,
        conversation::{Draft, Snapshot},
    };
    use std::sync::Arc;

    let mut snapshot = Snapshot {
        node: NodeId([1; 32]),
        sequence: 1,
        page: Arc::new(page()),
        missing: Vec::new(),
        drafts: Vec::new(),
        statistics: Default::default(),
    };
    let mut view = View::default();
    view.replace(Arc::new(snapshot.clone()));
    let calls = view.calls.clone();
    snapshot.sequence += 1;
    snapshot.drafts.push(Draft {
        id: "draft".into(),
        turn: snapshot.page.runs[0].turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Text("streaming".into())],
    });
    view.replace(Arc::new(snapshot.clone()));
    assert!(Arc::ptr_eq(&calls, &view.calls));
    let turn = snapshot.page.runs[0].turn;
    Arc::make_mut(&mut snapshot.page)
        .entries
        .push(entry(2, turn, "", true));
    view.replace(Arc::new(snapshot));
    assert!(!Arc::ptr_eq(&calls, &view.calls));
    assert_eq!(view.calls[0].state, State::Returned);
    let serialized = serde_json::to_value(&view).unwrap();
    assert_eq!(serialized["calls"][0]["response"]["entry"], "event-2");
    assert!(serialized["calls"][0].get("result").is_none());
}

#[test]
fn starts_only_the_matching_call() {
    let mut page = page();
    let turn = page.runs[0].turn;
    let mut queued = entry(2, turn, "", false);
    if let Part::ToolCall { id, .. } = &mut queued.parts[0] {
        *id = Some("later".into());
    }
    page.entries.push(queued);
    let mut started = entry(3, turn, "", false);
    started.parts = vec![Part::Resource(
        json!({"type":"tool_started", "id":"call", "name":"read_file"}),
    )];
    page.entries.push(started);
    assert_eq!(
        collect(&page)
            .iter()
            .map(|call| call.state)
            .collect::<Vec<_>>(),
        [State::Running, State::Waiting]
    );
    page.entries.push(entry(4, turn, "", true));
    assert_eq!(
        collect(&page)
            .iter()
            .map(|call| call.state)
            .collect::<Vec<_>>(),
        [State::Returned, State::Waiting]
    );
    page.runs[0].status = Status::Cancelled;
    assert_eq!(collect(&page)[1].state, State::Cancelled);
}

#[test]
fn preserves_grouping_before_and_after_results() {
    use sailry_protocol::tool::{Grouping, Presentation};
    let mut page = page();
    if let Part::ToolCall { grouping, .. } = &mut page.entries[0].parts[0] {
        *grouping = Grouping::Standalone;
    }
    assert_eq!(collect(&page)[0].grouping, Grouping::Standalone);
    page.entries.push(entry(2, page.runs[0].turn, "", true));
    let call = &collect(&page)[0];
    assert_eq!(call.grouping, Grouping::Standalone);
    assert_eq!(
        serde_json::to_value(call).unwrap()["grouping"],
        "standalone"
    );
    if let Part::ToolCall {
        grouping,
        presentation,
        ..
    } = &mut page.entries[0].parts[0]
    {
        *grouping = Grouping::Sequence;
        *presentation = Presentation::Progress;
    }
    page.entries.pop();
    assert_eq!(collect(&page)[0].grouping, Grouping::Standalone);
}
