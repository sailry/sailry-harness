use super::*;
use core::prelude::v1::test;
use sailry_client::conversation::tools::{Reference, State};
use sailry_protocol::{
    NodeId, SessionId,
    conversation::{Entry, Page, Snapshot},
};
use std::sync::Arc;

mod chronology;
mod compact;
mod failure;
mod progress;
mod retry;

#[test]
fn keeps_native_references_in_canonical_copy_source() {
    let turn = TurnId::new();
    let source = "Result\u{e200}cite\u{e202}turn0search2\u{e201} tail";
    let history = View {
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
                background: Vec::new(),
                runs: vec![],
                entries: vec![Entry {
                    sequence: 1,
                    id: "answer".into(),
                    turn,
                    author: "assistant".into(),
                    branch: String::new(),
                    timestamp_ms: 1,
                    parts: vec![Part::Text(source.into())],
                    usage: None,
                    citations: vec![],
                    search_suggestions: None,
                }],
            }),
        })),
        ..View::default()
    };
    let blocks = collect(turn, &history);
    let Block::Text(_, copy) = &blocks[0] else {
        panic!("text block expected")
    };
    assert_eq!(copy, source);
    assert_eq!(super::super::citations::presentation(copy), "Result tail");
    assert_eq!(
        history.snapshot.as_ref().unwrap().page.entries[0].parts,
        vec![Part::Text(source.into())]
    );
}

#[test]
fn preserves_image_message_boundaries() {
    use sailry_protocol::{AttachmentId, WorktreeId, attachment, conversation::Image};
    let turn = TurnId::new();
    let image = |index| {
        Part::Image(Image {
            entry: "source".into(),
            part: index,
            index,
            attachment: attachment::Attachment {
                id: AttachmentId::new(),
                spec: attachment::Spec {
                    worktree: WorktreeId::new(),
                    name: format!("image-{index}.png"),
                    media_type: "image/png".into(),
                    size: 1,
                    revision: "fixture".into(),
                },
            },
        })
    };
    let entries = [
        vec![image(0), image(1)],
        vec![image(2), Part::Text("Between".into()), image(3)],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, parts)| Entry {
        sequence: index as u64 + 1,
        id: format!("entry-{index}"),
        turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts,
        usage: None,
        citations: vec![],
        search_suggestions: None,
    })
    .collect();
    let history = View {
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
                background: Vec::new(),
                runs: vec![],
                entries,
            }),
        })),
        ..View::default()
    };
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 4);
    assert!(
        matches!(&blocks[0], Block::Images(images) if images.len() == 2 && images[0].index == 0 && images[1].index == 1)
    );
    assert!(
        matches!(&blocks[1], Block::Images(images) if images.len() == 1 && images[0].index == 2)
    );
    assert!(matches!(&blocks[2], Block::Text(_, text) if text == "Between"));
    assert!(
        matches!(&blocks[3], Block::Images(images) if images.len() == 1 && images[0].index == 3)
    );
}

#[test]
fn retains_text_boundaries() {
    let turn = TurnId::new();
    let parts = vec![
        Part::ToolCall {
            display: None,
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some("a".into()),
            name: "read_file".into(),
            arguments: serde_json::json!({}),
        },
        Part::ToolResult {
            id: Some("a".into()),
            name: "read_file".into(),
            result: serde_json::json!("result"),
            images: vec![],
        },
        Part::Text("between tools".into()),
        Part::ToolResult {
            id: None,
            name: "external".into(),
            result: serde_json::json!("orphan result"),
            images: vec![],
        },
        Part::Thinking("reasoning".into()),
        Part::Resource(serde_json::json!({"type":"provider_context"})),
        Part::Compaction("Summary".into()),
        Part::Resource(
            serde_json::json!({"type":"operation_warning","code":"context_summary_failed","turn":turn}),
        ),
    ];
    let reference = |index| Reference {
        entry: "entry".into(),
        index,
    };
    let mut history = View {
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: Vec::new(),
            drafts: Vec::new(),
            statistics: Default::default(),
            artifacts: Default::default(),
            page: Arc::new(Page {
                session: SessionId::new(),
                revision: 1,
                next_before: None,
                queue: Default::default(),
                approvals: Vec::new(),
                questions: Vec::new(),
                children: Vec::new(),
                background: Vec::new(),
                runs: Vec::new(),
                entries: vec![Entry {
                    sequence: 1,
                    id: "entry".into(),
                    turn,
                    author: "assistant".into(),
                    branch: String::new(),
                    timestamp_ms: 1,
                    parts,
                    usage: None,
                    citations: vec![],
                    search_suggestions: None,
                }],
            }),
        })),
        calls: Arc::new(vec![
            Call {
                terminal_interaction: false,
                presentation: Default::default(),
                grouping: Default::default(),
                turn,
                id: Some("a".into()),
                name: "read_file".into(),
                progress: None,
                content: None,
                resolved: None,
                source: reference(0),
                response: Some(reference(1)),
                state: State::Returned,
                approval: None,
                question: None,
            },
            Call {
                terminal_interaction: false,
                presentation: Default::default(),
                grouping: Default::default(),
                turn,
                id: None,
                name: "external".into(),
                progress: None,
                content: None,
                resolved: None,
                source: reference(3),
                response: Some(reference(3)),
                state: State::Returned,
                approval: None,
                question: None,
            },
        ]),
        ..View::default()
    };
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 5);
    assert!(matches!(&blocks[0], Block::Tools(calls) if calls.len() == 1));
    assert!(matches!(&blocks[1], Block::Text(_, text) if text == "between tools"));
    assert!(matches!(&blocks[2], Block::Tools(calls) if calls.len() == 1 && calls[0].id.is_none()));
    assert!(matches!(&blocks[3], Block::Thinking(_, text) if text == "reasoning"));
    assert!(matches!(&blocks[4], Block::Compaction(_, "Summary")));
    drop(blocks);
    let snapshot = Arc::make_mut(history.snapshot.as_mut().unwrap());
    Arc::make_mut(&mut snapshot.page).entries[0].parts[2] = Part::Text(" \n\t".into());
    snapshot.drafts.push(sailry_protocol::conversation::Draft {
        id: "blank".into(),
        turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Text(" ".into())],
    });
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], Block::Tools(calls) if calls.len() == 2));
    assert!(!blocks.iter().any(|block| matches!(block, Block::Text(..))));
    drop(blocks);
    for index in 0..2 {
        Arc::make_mut(&mut history.calls)[index].progress = Some(
            serde_json::from_value(serde_json::json!({
                "title": null,
                "steps": [{"description": "Verify", "state": "completed"}]
            }))
            .unwrap(),
        );
        let blocks = collect(turn, &history);
        assert_eq!(blocks.len(), 4);
        assert!(matches!(&blocks[0], Block::Tools(calls) if calls.len() == 1));
        assert!(matches!(&blocks[1], Block::Tools(calls) if calls.len() == 1));
        drop(blocks);
        Arc::make_mut(&mut history.calls)[index].progress = None;
    }
    for index in 0..2 {
        Arc::make_mut(&mut history.calls)[index].grouping =
            sailry_protocol::tool::Grouping::Standalone;
        let blocks = collect(turn, &history);
        assert!(matches!(&blocks[0], Block::Tools(calls) if calls.len() == 1));
        assert!(matches!(&blocks[1], Block::Tools(calls) if calls.len() == 1));
        drop(blocks);
        Arc::make_mut(&mut history.calls)[index].grouping = Default::default();
    }
    let snapshot = Arc::make_mut(history.snapshot.as_mut().unwrap());
    let page = Arc::make_mut(&mut snapshot.page);
    page.entries[0].parts = vec![
        Part::Thinking("Think ".into()),
        Part::Thinking("together".into()),
        Part::Text("SA".into()),
        Part::Text("IL".into()),
        Part::Text(" ".into()),
        Part::Text("RY_OK".into()),
    ];
    let mut next = page.entries[0].clone();
    next.id = "next-response".into();
    next.sequence = 2;
    next.parts = vec![
        Part::Resource(serde_json::json!({"type":"provider_context"})),
        Part::Text("Separate response".into()),
    ];
    page.entries.push(next);
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], Block::Thinking(_, text) if text == "Think together"));
    assert!(
        matches!(&blocks[1], Block::Text(key, text) if key == "entry-2" && text == "SAIL RY_OK")
    );
    assert!(matches!(&blocks[2], Block::Text(_, text) if text == "Separate response"));
}
