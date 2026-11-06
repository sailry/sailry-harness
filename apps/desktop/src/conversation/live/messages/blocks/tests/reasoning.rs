use super::*;
use crate::conversation::live::messages::work;
use sailry_protocol::conversation::Draft;

fn history(turn: TurnId, parts: Vec<Part>) -> View {
    View {
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
            page: Arc::new(Page {
                session: SessionId::new(),
                revision: 1,
                next_before: None,
                queue: Default::default(),
                approvals: vec![],
                questions: vec![],
                children: vec![],
                background: vec![],
                runs: vec![],
                entries: vec![Entry {
                    sequence: 1,
                    id: "answer".into(),
                    turn,
                    author: "assistant".into(),
                    branch: String::new(),
                    timestamp_ms: 1,
                    parts,
                    usage: None,
                    citations: vec![],
                    search_suggestions: None,
                }]
                .into(),
            }),
        })),
        ..View::default()
    }
}

#[test]
fn keeps_answers_outside_collapsed_work() {
    for suffix in [
        vec![],
        vec![Part::Resource(
            serde_json::json!({"type":"provider_context"}),
        )],
        vec![Part::Text(" \n\t".into())],
    ] {
        for draft in [false, true] {
            let turn = TurnId::new();
            let mut parts = vec![
                Part::Text("A🙂".into()),
                Part::Text("中".into()),
                Part::Thinking("Delayed thought".into()),
            ];
            parts.extend(suffix.clone());
            let mut view = history(turn, parts.clone());
            if draft {
                let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries).clear();
                snapshot.drafts.push(Draft {
                    id: "answer".into(),
                    turn,
                    author: "sailry".into(),
                    branch: String::new(),
                    parts,
                });
            }
            let original = view.snapshot.clone();
            let blocks = super::super::compact(collect(turn, &view));
            assert_eq!(blocks.len(), 1);
            assert!(matches!(&blocks[0], Block::Text(key, text)
                if text == "A🙂中" && key == if draft { "draft-answer-0" } else { "answer-0" }));
            assert_eq!(work::answer_from(&blocks), 0);
            assert_eq!(view.snapshot, original);
        }
    }
}

#[test]
fn preserves_streaming_text_positions() {
    let turn = TurnId::new();
    let mut view = history(turn, vec![]);
    let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
    Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries).clear();
    snapshot.drafts.push(Draft {
        id: "answer".into(),
        turn,
        author: "sailry".into(),
        branch: String::new(),
        parts: vec![],
    });
    for (part, expected) in [
        (Part::Text("First".into()), vec![(0, "First")]),
        (Part::Thinking("Thinking".into()), vec![(0, "First")]),
        (
            Part::Text("Second".into()),
            vec![(0, "First"), (2, "Second")],
        ),
        (
            Part::Thinking("More".into()),
            vec![(0, "First"), (2, "Second")],
        ),
    ] {
        Arc::make_mut(view.snapshot.as_mut().unwrap()).drafts[0]
            .parts
            .push(part);
        let original = view.snapshot.clone();
        let blocks = super::super::compact(collect(turn, &view));
        assert_eq!(blocks.len(), expected.len());
        for (block, (index, expected)) in blocks.iter().zip(expected) {
            assert!(matches!(block, Block::Text(key, text)
                if key == &format!("draft-answer-{index}") && text == expected));
        }
        assert_eq!(work::answer_from(&blocks), 0);
        assert_eq!(view.snapshot, original);
    }
    let parts = view.snapshot.as_ref().unwrap().drafts[0].parts.clone();
    let committed = history(turn, parts);
    let blocks = super::super::compact(collect(turn, &committed));
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], Block::Text(key, text) if key == "answer-0" && text == "First"));
    assert!(matches!(&blocks[1], Block::Text(key, text) if key == "answer-2" && text == "Second"));
    assert_eq!(work::answer_from(&blocks), 0);
}

#[test]
fn ignores_thought_only_followup_entries() {
    let turn = TurnId::new();
    let mut view = history(turn, vec![Part::Text("Answer".into())]);
    let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
    let entries = Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries);
    let mut next = entries[0].clone();
    next.id = "later".into();
    next.sequence = 2;
    next.parts = vec![Part::Thinking("Separate thought".into())];
    entries.push(next);
    let original = view.snapshot.clone();
    let blocks = super::super::compact(collect(turn, &view));
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks[0], Block::Text(key, text) if key == "answer-0" && text == "Answer"));
    assert_eq!(work::answer_from(&blocks), 0);
    assert_eq!(view.snapshot, original);
}

#[test]
fn preserves_tool_and_answer_boundaries() {
    let turn = TurnId::new();
    let mut view = history(
        turn,
        vec![
            Part::Text("Progress".into()),
            Part::Thinking("Before the tool".into()),
            Part::ToolResult {
                id: Some("read".into()),
                name: "read_file".into(),
                summary: Default::default(),
                images: vec![],
            },
            Part::Text("Final answer".into()),
            Part::Thinking("Delayed thought".into()),
        ],
    );
    let source = Reference {
        entry: "answer".into(),
        index: 2,
    };
    view.calls = Arc::new(vec![Call {
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        turn,
        id: Some("read".into()),
        name: "read_file".into(),
        progress: None,
        resolved: None,
        detail: None,
        source: source.clone(),
        response: Some(source),
        state: State::Returned,
        failure: None,
        approval: None,
        question: None,
    }]);
    let original = view.snapshot.clone();
    let blocks = super::super::compact(collect(turn, &view));
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], Block::Text(_, text) if text == "Progress"));
    assert!(matches!(&blocks[1], Block::Tools(calls)
        if calls.len() == 1 && calls[0].source.index == 2));
    assert!(
        matches!(&blocks[2], Block::Text(key, text) if key == "answer-3" && text == "Final answer")
    );
    assert_eq!(work::answer_from(&blocks), 2);
    assert_eq!(view.snapshot, original);
    drop(blocks);
    let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
    Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries)[0]
        .parts
        .truncate(3);
    let blocks = super::super::compact(collect(turn, &view));
    assert_eq!(work::answer_from(&blocks), blocks.len());
}
