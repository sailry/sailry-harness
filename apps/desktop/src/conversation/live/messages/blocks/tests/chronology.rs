use super::*;
use sailry_protocol::{
    Delegation,
    conversation::{Child, Run, RunKind, Status},
};
use serde_json::json;

#[test]
fn delegation_order() {
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
    let ids: Vec<_> = children.iter().map(|child| child.run.session).collect();
    let mut history = View {
        calls: Arc::new(calls),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            page: Arc::new(Page {
                session,
                revision: 1,
                next_before: None,
                queue: Default::default(),
                approvals: vec![],
                questions: vec![],
                children,
                runs: vec![],
                entries,
            }),
        })),
        ..Default::default()
    };
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], Block::Children(group) if group == &ids[..2]));
    assert!(matches!(&blocks[1], Block::Compaction(_, "Summary")));
    assert!(matches!(&blocks[2], Block::Children(group) if group == &ids[2..]));

    // Missing summaries must not hide the tool's result or error.
    Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page)
        .entries
        .retain(|entry| entry.id != "summary");
    let blocks = collect(turn, &history);
    assert!(matches!(&blocks[1], Block::Tools(group) if group[0].name == "compact_context"));
}
