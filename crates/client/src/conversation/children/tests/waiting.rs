//! Foreground delegation holds the parent's next model request.
use crate::conversation::{View, guidance, tools, transcript, turns};
use sailry_protocol::{
    ApprovalId, Delegation, NodeId, QuestionId, SessionId, TurnId, WorktreeId,
    conversation::{
        Approval, ApprovalSource, ApprovalState, Child, Draft, Entry, Page, Part, Run, RunKind,
        SentInput, Snapshot, Status, question,
    },
    tool::detail::{self, Summary},
};
use std::sync::Arc;

fn parent() -> Run {
    Run {
        worktree: WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1),
        finished_ms: None,
        origin: None,
        continuation: None,
        guidance: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    }
}

fn entry(sequence: u64, turn: TurnId, author: &str, parts: Vec<Part>) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn,
        author: author.into(),
        branch: String::new(),
        timestamp_ms: sequence as i64,
        parts,
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    }
}

fn call(id: &str, name: &str, background: bool) -> Part {
    Part::ToolCall {
        id: Some(id.into()),
        name: name.into(),
        summary: Box::new(Summary {
            delegation: detail::Delegation {
                background,
                ..Default::default()
            },
            ..Default::default()
        }),
        presentation: Default::default(),
        grouping: Default::default(),
    }
}

fn result(id: &str, name: &str) -> Part {
    Part::ToolResult {
        id: Some(id.into()),
        name: name.into(),
        summary: Box::default(),
        images: Vec::new(),
    }
}

fn child(parent: &Run, index: usize, status: Status, sequence: u64) -> Child {
    Child {
        run: Run {
            turn: TurnId::new(),
            session: SessionId::new(),
            sequence,
            status,
            started_ms: None,
            ..parent.clone()
        },
        origin: Delegation {
            session: parent.session,
            turn: parent.turn,
            entry: "entry-1".into(),
            index,
            role: None,
        },
        name: None,
    }
}

/// A running parent whose reply delegated one running and one queued child.
fn delegated(background: [bool; 2]) -> (Run, Page) {
    let parent = parent();
    let page = Page {
        session: parent.session,
        revision: 1,
        entries: Arc::new(vec![
            entry(
                1,
                parent.turn,
                "assistant",
                vec![
                    Part::Text("Delegating".into()),
                    call("first", "spawn_agent", background[0]),
                    call("second", "spawn_agent", background[1]),
                ],
            ),
            entry(
                2,
                parent.turn,
                "tool",
                vec![
                    result("first", "spawn_agent"),
                    result("second", "spawn_agent"),
                ],
            ),
        ]),
        runs: vec![parent.clone()],
        queue: Default::default(),
        approvals: Vec::new(),
        questions: Vec::new(),
        children: vec![
            child(&parent, 1, Status::Running, 2),
            child(&parent, 2, Status::Queued, 3),
        ],
        background: Vec::new(),
        next_before: None,
    };
    (parent, page)
}

/// The waiting count each segment of the parent's reply reports.
fn reported(page: &Page, drafts: Vec<Draft>, turn: TurnId) -> Vec<Option<usize>> {
    let view = View {
        calls: Arc::new(tools::collect(page)),
        guidance: Arc::new(guidance::project(page)),
        timeline: Arc::new(turns::timeline(page)),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            page: Arc::new(page.clone()),
            missing: Vec::new(),
            drafts,
            statistics: None,
            artifacts: Default::default(),
        })),
        ..Default::default()
    };
    transcript::segments(&view, turn)
        .iter()
        .map(|segment| segment.waiting)
        .collect()
}

fn append(page: &mut Page, entries: impl IntoIterator<Item = Entry>) {
    Arc::make_mut(&mut page.entries).extend(entries);
}

/// Guidance for `parent`; delivery joins it and records its canonical input.
fn guide(page: &mut Page, parent: &Run, delivered: bool) {
    let guide = Run {
        turn: TurnId::new(),
        sequence: 2,
        status: if delivered {
            Status::Continued
        } else {
            Status::Queued
        },
        started_ms: None,
        finished_ms: delivered.then_some(3),
        guidance: Some(parent.turn),
        joined: delivered.then_some(parent.turn),
        input: Some(SentInput {
            message: "Check the fixture".into(),
            attachments: Vec::new(),
        }),
        ..parent.clone()
    };
    if delivered {
        let sequence = page.entries.len() as u64 + 1;
        append(
            page,
            [entry(
                sequence,
                guide.turn,
                "user",
                vec![Part::Text("Check the fixture".into())],
            )],
        );
    }
    page.runs.push(guide);
}

#[test]
fn counts_unfinished_foreground_children() {
    let (parent, mut page) = delegated([false, false]);
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(2)]);
    page.children[0].run.status = Status::Stopping;
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(2)]);
}

#[test]
fn clears_after_children_finish() {
    let (parent, mut page) = delegated([false, false]);
    page.children[0].run.status = Status::Completed;
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(1)]);
    page.children[1].run.status = Status::Failed;
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
}

#[test]
fn follows_model_replies() {
    let (parent, mut page) = delegated([false, false]);
    let draft = Draft {
        id: "draft".into(),
        turn: parent.turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Thinking("Reviewing".into())],
    };
    assert_eq!(reported(&page, vec![draft], parent.turn), [None]);
    guide(&mut page, &parent, true);
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None, None]);
    append(
        &mut page,
        [entry(
            4,
            parent.turn,
            "assistant",
            vec![Part::Text("Interim report".into())],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None, Some(2)]);
}

#[test]
fn clears_for_pending_guidance() {
    let (parent, mut page) = delegated([false, false]);
    guide(&mut page, &parent, false);
    assert_eq!(
        guidance::project(&page).values().collect::<Vec<_>>(),
        [&guidance::State::Pending]
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
}

#[test]
fn clears_for_retries() {
    let (parent, mut page) = delegated([false, false]);
    append(
        &mut page,
        [entry(
            3,
            parent.turn,
            "assistant",
            vec![Part::Resource(
                serde_json::json!({"type":"model_retry","attempt":1,"limit":5}),
            )],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
}

#[test]
fn ignores_background_children() {
    let (parent, page) = delegated([true, true]);
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
    let (parent, page) = delegated([true, false]);
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(1)]);
}

#[test]
fn requires_settled_running_parent() {
    let (parent, mut page) = delegated([false, false]);
    page.runs[0].status = Status::Stopping;
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);

    let (parent, mut page) = delegated([false, false]);
    page.approvals.push(Approval {
        id: ApprovalId::new(),
        session: parent.session,
        turn: parent.turn,
        entry: "entry-1".into(),
        index: 1,
        state: ApprovalState::Pending,
        source: ApprovalSource::User,
        choice: None,
        reason: None,
        prefix: None,
        can_remember: false,
        startup: None,
        ssh: None,
        access: None,
    });
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);

    let (parent, mut page) = delegated([false, false]);
    page.questions.push(question::Question {
        id: QuestionId::new(),
        session: parent.session,
        turn: parent.turn,
        entry: "entry-1".into(),
        index: 1,
        state: question::State::Pending,
    });
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);

    let (parent, mut page) = delegated([false, false]);
    page.runs[0].kind = RunKind::Compaction;
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);

    let (parent, mut page) = delegated([false, false]);
    append(
        &mut page,
        [entry(
            3,
            parent.turn,
            "assistant",
            vec![call("read", "read_file", false)],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
    append(
        &mut page,
        [entry(
            4,
            parent.turn,
            "tool",
            vec![result("read", "read_file")],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(2)]);
}

#[test]
fn ignores_other_turns() {
    let (parent, mut page) = delegated([false, false]);
    page.children[1].origin.turn = TurnId::new();
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(1)]);
    page.children[0].origin.turn = TurnId::new();
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
}

#[test]
fn clears_for_unmatched_calls() {
    let (parent, mut page) = delegated([false, false]);
    append(
        &mut page,
        [entry(
            3,
            parent.turn,
            "tool",
            vec![result("orphan", "read_file")],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [Some(2)]);
    append(
        &mut page,
        [entry(
            4,
            parent.turn,
            "assistant",
            vec![Part::ToolCall {
                id: None,
                name: "read_file".into(),
                summary: Box::default(),
                presentation: Default::default(),
                grouping: Default::default(),
            }],
        )],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None]);
}

#[test]
fn reports_latest_segment_only() {
    let (parent, mut page) = delegated([false, false]);
    guide(&mut page, &parent, true);
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None, None]);
    append(
        &mut page,
        [
            entry(
                4,
                parent.turn,
                "assistant",
                vec![call("read", "read_file", false)],
            ),
            entry(5, parent.turn, "tool", vec![result("read", "read_file")]),
        ],
    );
    assert_eq!(reported(&page, Vec::new(), parent.turn), [None, Some(2)]);
}
