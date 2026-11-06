use super::*;
use sailry_protocol::conversation::{Run, RunKind, SentInput, Status};

fn run(turn: TurnId, joined: Option<TurnId>) -> Run {
    Run {
        worktree: sailry_protocol::WorktreeId::new(),
        turn,
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: if joined.is_some() {
            Status::Continued
        } else {
            Status::Running
        },
        error: None,
        started_ms: Some(1),
        finished_ms: None,
        origin: None,
        continuation: None,
        guidance: joined,
        joined,
        notice: false,
        notices: Vec::new(),
        input: Some(SentInput {
            message: "Input".into(),
            attachments: Vec::new(),
        }),
    }
}

fn entry(sequence: u64, turn: TurnId, author: &str, parts: Vec<Part>) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn,
        author: author.into(),
        branch: String::new(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        parts,
    }
}

fn tool(id: &str) -> Vec<Part> {
    vec![
        Part::ToolCall {
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some(id.into()),
            name: "read_file".into(),
            summary: Default::default(),
        },
        Part::ToolResult {
            id: Some(id.into()),
            name: "read_file".into(),
            summary: Default::default(),
            images: vec![],
        },
    ]
}

fn call(turn: TurnId, entry: &str) -> Call {
    Call {
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        turn,
        id: Some(entry.into()),
        name: "read_file".into(),
        progress: None,
        resolved: None,
        detail: None,
        source: Reference {
            entry: entry.into(),
            index: 0,
        },
        response: Some(Reference {
            entry: entry.into(),
            index: 1,
        }),
        state: State::Returned,
        failure: None,
        approval: None,
        question: None,
    }
}

#[test]
fn keeps_tool_groups_inside_their_segment() {
    let owner = TurnId::new();
    let guide = TurnId::new();
    let page = Page {
        session: SessionId::new(),
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: Vec::new(),
        runs: vec![run(owner, None), run(guide, Some(owner))],
        entries: vec![
            entry(1, owner, "user", vec![Part::Text("Initial task".into())]),
            entry(2, owner, "assistant", tool("entry-2")),
            entry(3, guide, "user", vec![Part::Text("Use the fixture".into())]),
            entry(4, owner, "assistant", tool("entry-4")),
            entry(5, owner, "assistant", vec![Part::Text("Reply".into())]),
        ]
        .into(),
    };
    let history = View {
        calls: Arc::new(vec![call(owner, "entry-2"), call(owner, "entry-4")]),
        guidance: Arc::new(sailry_client::conversation::guidance::project(&page)),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
            page: Arc::new(page),
        })),
        ..View::default()
    };
    let segments = super::super::segments(owner, &history);
    assert_eq!(
        segments
            .iter()
            .map(|segment| segment.inputs.clone())
            .collect::<Vec<_>>(),
        [vec![owner], vec![guide]]
    );
    assert!(matches!(
        &segments[0].blocks[..],
        [Block::Tools(calls)] if calls.len() == 1
    ));
    assert!(matches!(
        &segments[1].blocks[..],
        [Block::Tools(calls), Block::Text(_, text)] if calls.len() == 1 && text == "Reply"
    ));
}
