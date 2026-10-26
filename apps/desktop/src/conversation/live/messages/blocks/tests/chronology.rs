use super::*;
use sailry_protocol::{
    Delegation,
    conversation::{Child, Run, RunKind, Status},
};
use serde_json::json;

fn history() -> (TurnId, View) {
    let turn = TurnId::new();
    let session = SessionId::new();
    let mut entries = Vec::new();
    let mut calls = Vec::new();
    let mut children = Vec::new();
    for (index, name) in [
        "third_party_delegate",
        "third_party_delegate",
        "compact_context",
        "third_party_delegate",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("entry-{index}");
        entries.push(Entry {
            id: id.clone(),
            sequence: index as u64 + 1,
            turn,
            author: "assistant".into(),
            branch: String::new(),
            timestamp_ms: 1,
            usage: None,
            citations: vec![],
            search_suggestions: None,
            parts: vec![
                Part::ToolCall {
                    display: None,
                    presentation: Default::default(),
                    grouping: Default::default(),
                    id: Some(id.clone()),
                    name: name.into(),
                    arguments: json!({}),
                },
                Part::ToolResult {
                    id: Some(id.clone()),
                    name: name.into(),
                    result: json!({"status": "compacted", "summary_id": "summary"}),
                    images: vec![],
                },
            ],
        });
        calls.push(Call {
            terminal_interaction: false,
            presentation: Default::default(),
            grouping: Default::default(),
            turn,
            id: Some(id.clone()),
            name: name.into(),
            progress: None,
            content: None,
            resolved: None,
            source: Reference {
                entry: id.clone(),
                index: 0,
            },
            response: Some(Reference {
                entry: id.clone(),
                index: 1,
            }),
            state: State::Returned,
            failure: None,
            approval: None,
            question: None,
        });
        if name == "third_party_delegate" {
            children.push(Child {
                run: Run {
                    worktree: sailry_protocol::WorktreeId::new(),
                    turn: TurnId::new(),
                    session: SessionId::new(),
                    kind: RunKind::Task,
                    sequence: index as u64 + 1,
                    revision: 1,
                    status: Status::Completed,
                    error: None,
                    started_ms: Some(1),
                    finished_ms: Some(2),
                    origin: None,
                    continuation: None,
                    joined: None,
                    notice: false,
                    notices: Vec::new(),
                    input: None,
                },
                origin: Delegation {
                    session,
                    turn,
                    entry: id,
                    index: 0,
                    role: None,
                },
                name: None,
            });
        } else {
            let mut summary = entries.last().unwrap().clone();
            summary.id = "summary".into();
            summary.parts = vec![Part::Compaction("Summary".into())];
            entries.push(summary);
        }
    }
    let history = View {
        calls: Arc::new(calls),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
            page: Arc::new(Page {
                session,
                revision: 1,
                next_before: None,
                queue: Default::default(),
                approvals: vec![],
                questions: vec![],
                children,
                background: Vec::new(),
                runs: vec![],
                entries,
            }),
        })),
        ..Default::default()
    };
    (turn, history)
}

#[test]
fn delegation_order() {
    let (turn, mut history) = history();
    let ids: Vec<_> = history
        .snapshot
        .as_ref()
        .unwrap()
        .page
        .children
        .iter()
        .map(|child| child.run.session)
        .collect();
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 3);
    assert!(
        matches!(&blocks[0], Block::Children(group) if group.iter().map(|child| child.run.session).eq(ids[..2].iter().copied()))
    );
    assert!(matches!(&blocks[1], Block::Compaction(_, "Summary")));
    assert!(
        matches!(&blocks[2], Block::Children(group) if group.iter().map(|child| child.run.session).eq(ids[2..].iter().copied()))
    );

    // Missing summaries must not hide the tool's result or error.
    Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page)
        .entries
        .retain(|entry| entry.id != "summary");
    let blocks = collect(turn, &history);
    assert!(matches!(&blocks[1], Block::Tools(group) if group[0].name == "compact_context"));
}

#[test]
fn keeps_reused_calls_independent() {
    let (turn, mut history) = history();
    let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
    page.children[0].run.status = Status::Cancelled;
    let original = page.children[0].run.turn;
    let mut guidance = page.children[0].clone();
    guidance.run.turn = TurnId::new();
    guidance.run.status = Status::Queued;
    guidance.run.sequence = 10;
    let source = guidance.run.turn;
    page.children[1].run.session = guidance.run.session;
    page.children[1].run.status = Status::Running;
    page.children[1].run.sequence = 11;
    let reused = page.children[1].run.turn;
    page.children.push(guidance);
    let blocks = collect(turn, &history);
    let Block::Children(children) = &blocks[0] else {
        panic!("child group expected")
    };
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].run.turn, original);
    assert_eq!(children[0].run.status, Status::Cancelled);
    assert_eq!(children[1].run.turn, reused);
    assert_eq!(children[1].run.status, Status::Running);
    drop(blocks);
    let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
    page.children.last_mut().unwrap().run.status = Status::Running;
    let blocks = collect(turn, &history);
    assert!(
        matches!(&blocks[0], Block::Children(children) if children[0].run.turn == source && children[1].run.turn == reused)
    );
    drop(blocks);
    let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
    let mut joined = page.children.last().unwrap().clone();
    joined.run.turn = TurnId::new();
    joined.run.sequence = 12;
    joined.run.status = Status::Continued;
    joined.run.joined = Some(source);
    page.children.push(joined);
    let blocks = collect(turn, &history);
    assert!(
        matches!(&blocks[0], Block::Children(children) if children[0].run.turn == source && children[1].run.turn == reused)
    );
}

#[test]
fn joins_children_after_hidden_reasoning() {
    let (turn, mut history) = history();
    let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
    page.entries[0]
        .parts
        .push(Part::Thinking("Before the next delegation".into()));
    let original = page.clone();
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 5);
    assert!(matches!(&blocks[1], Block::Thinking(..)));
    let blocks = super::super::compact(blocks);
    assert_eq!(blocks.len(), 3);
    assert!(
        matches!(&blocks[0], Block::Children(children) if children.iter().map(|child| child.run.session).eq(original.children[..2].iter().map(|child| child.run.session)))
    );
    assert!(matches!(&blocks[1], Block::Compaction(..)));
    assert!(
        matches!(&blocks[2], Block::Children(children) if children.len() == 1 && children[0].run.session == original.children[2].run.session)
    );
    assert_eq!(history.snapshot.as_ref().unwrap().page.as_ref(), &original);
}

#[test]
fn keeps_children_at_visible_boundaries() {
    let (_, history) = history();
    let children = &history.snapshot.as_ref().unwrap().page.children;
    let call = &history.calls[2];
    let mut progress = (*call).clone();
    progress.presentation = sailry_protocol::tool::Presentation::Progress;
    for separator in [
        Block::Text("answer".into(), "Keep this answer here".into()),
        Block::Tools(vec![call]),
        Block::Tools(vec![&progress]),
        Block::Question(call),
        Block::Resource,
        Block::Compaction("summary".into(), "Summary"),
    ] {
        let blocks = super::super::compact(vec![
            Block::Children(vec![&children[0]]),
            Block::Thinking("before".into(), "Before".into()),
            separator,
            Block::Thinking("after".into(), "After".into()),
            Block::Children(vec![&children[1]]),
        ]);
        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], Block::Children(group) if group.len() == 1 && std::ptr::eq(group[0], &children[0]))
        );
        assert!(
            matches!(&blocks[2], Block::Children(group) if group.len() == 1 && std::ptr::eq(group[0], &children[1]))
        );
    }
}
