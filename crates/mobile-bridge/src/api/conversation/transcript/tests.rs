use super::*;
use sailry_protocol::{
    AttachmentId, NodeId, SessionId, WorktreeId,
    attachment::{Attachment, Spec},
    conversation::{
        Draft, Entry, Page, Part, Pending, Run, RunKind, SentInput, Snapshot, Status,
        reference::{Reference, Target},
    },
};
use serde_json::json;
use std::sync::Arc;

fn entry(turn: TurnId, sequence: u64, author: &str, parts: Vec<Part>) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn,
        author: author.into(),
        branch: String::new(),
        timestamp_ms: sequence as i64,
        parts,
        citations: vec![],
        search_suggestions: None,
        usage: None,
    }
}

fn fixture() -> Page {
    let run = Run {
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
        notices: vec![],
        input: Some(SentInput {
            message: "Original task".into(),
            attachments: vec![],
        }),
    };
    Page {
        session: run.session,
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: vec![],
        entries: vec![entry(
            run.turn,
            1,
            "user",
            vec![Part::Text("Original task".into())],
        )]
        .into(),
        runs: vec![run],
    }
}

fn encoded(page: &Page, drafts: Vec<Draft>) -> Value {
    let view = View {
        connected: true,
        timeline: Arc::new(turns::timeline(page)),
        guidance: Arc::new(guidance::project(page)),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            page: Arc::new(page.clone()),
            missing: vec![],
            drafts,
            statistics: None,
            artifacts: Default::default(),
        })),
        ..Default::default()
    };
    serde_json::from_str(&super::super::tools::view(&view, None, &mut Default::default()).unwrap())
        .unwrap()
}

fn joined(primary: &Run, sequence: u64, message: &str) -> Run {
    Run {
        turn: TurnId::new(),
        sequence,
        status: Status::Continued,
        joined: Some(primary.turn),
        guidance: Some(primary.turn),
        started_ms: None,
        finished_ms: Some(sequence as i64),
        input: Some(SentInput {
            message: message.into(),
            attachments: vec![],
        }),
        ..primary.clone()
    }
}

#[test]
fn groups_inputs_in_canonical_order() {
    let mut page = fixture();
    let mut primary = page.runs[0].clone();
    primary.sequence = 3;
    let first = joined(&primary, 1, "Earlier admitted guide");
    let second = joined(&primary, 2, "Superseded capture");
    let attachment = Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: primary.worktree,
            name: "original.png".into(),
            media_type: "image/png".into(),
            size: 64,
            revision: "fixture-image-digest".into(),
        },
    };
    let reference = Reference {
        target: Target::Project,
        label: "Captured project".into(),
    };
    let text = "  Guidance 中文 🙂\nOriginal spacing  ";
    page.runs = vec![first.clone(), second.clone(), primary.clone()];
    Arc::make_mut(&mut page.entries).extend([
        entry(
            primary.turn,
            2,
            "assistant",
            vec![Part::Text("Work".into())],
        ),
        entry(
            second.turn,
            3,
            "user",
            vec![
                Part::Text(text.into()),
                Part::Attachment(attachment.clone()),
                Part::Reference(reference.clone()),
            ],
        ),
        entry(
            primary.turn,
            4,
            "assistant",
            vec![Part::Text("More work".into())],
        ),
        entry(
            first.turn,
            5,
            "user",
            vec![Part::Text("Earlier admitted guide".into())],
        ),
    ]);
    let value = encoded(&page, vec![]);
    let inputs = &value["inputs"][primary.turn.to_string()];
    assert_eq!(value["timeline"], json!([primary.turn]));
    assert_eq!(value["owners"][first.turn.to_string()], json!(primary.turn));
    assert_eq!(
        value["owners"][second.turn.to_string()],
        json!(primary.turn)
    );
    assert_eq!(
        inputs,
        &json!([
            {
                "turn": primary.turn,
                "message": {"text":"Original task", "attachments":[]},
                "attachments":[],
                "canonical":true,
            },
            {
                "turn": second.turn,
                "message": {
                    "text":text,
                    "attachments":[attachment.id],
                    "references":[reference],
                },
                "attachments":[attachment],
                "canonical":true,
            },
            {
                "turn": first.turn,
                "message": {"text":"Earlier admitted guide", "attachments":[]},
                "attachments":[],
                "canonical":true,
            },
        ])
    );
    assert_eq!(
        value["rows"][primary.turn.to_string()]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        value["snapshot"]["page"],
        serde_json::to_value(&page).unwrap()
    );
}

#[test]
fn waits_for_canonical_guidance_and_claim() {
    for canonical_first in [false, true] {
        let mut page = fixture();
        let primary = page.runs[0].clone();
        let mut guide = joined(&primary, 2, "Guidance capture");
        guide.status = Status::Queued;
        guide.joined = None;
        guide.finished_ms = None;
        page.queue.items.push(Pending {
            turn: guide.turn,
            revision: 1,
            config_revision: 1,
            ready: true,
            guidance: Some(primary.turn),
            notice: false,
            request: None,
            kind: RunKind::Task,
            preview: "Guidance capture".into(),
            truncated: false,
            attachments: vec![],
        });
        page.runs.push(guide.clone());
        for step in 0..3 {
            if step > 0 {
                if canonical_first == (step == 1) {
                    Arc::make_mut(&mut page.entries).push(entry(
                        guide.turn,
                        2,
                        "user",
                        vec![Part::Text("Canonical guidance".into())],
                    ));
                } else {
                    page.runs[1].joined = Some(primary.turn);
                    page.runs[1].status = Status::Continued;
                    page.runs[1].finished_ms = Some(2);
                }
            }
            let value = encoded(&page, vec![]);
            assert_eq!(value["timeline"], json!([primary.turn]));
            let inputs = value["inputs"][primary.turn.to_string()]
                .as_array()
                .unwrap();
            assert_eq!(inputs.len(), if step == 2 { 2 } else { 1 });
            assert_eq!(value["snapshot"]["page"]["queue"], json!(page.queue));
        }
        page.queue.items.clear();
        let draft = Draft {
            id: "stream".into(),
            turn: primary.turn,
            author: "assistant".into(),
            branch: String::new(),
            parts: vec![Part::Text("Streaming answer".into())],
        };
        let streaming = encoded(&page, vec![draft]);
        Arc::make_mut(&mut page.entries).push(entry(
            primary.turn,
            3,
            "assistant",
            vec![Part::Text("Streaming answer".into())],
        ));
        let committed = encoded(&page, vec![]);
        let reconnected = encoded(&page.clone(), vec![]);
        for value in [&streaming, &committed, &reconnected] {
            assert_eq!(value["timeline"], json!([primary.turn]));
            let inputs = value["inputs"][primary.turn.to_string()]
                .as_array()
                .unwrap();
            assert_eq!(inputs.len(), 2);
            assert_eq!(inputs[1]["turn"], json!(guide.turn));
            assert_eq!(inputs[1]["message"]["text"], "Canonical guidance");
            assert_eq!(inputs[1]["canonical"], true);
            assert_eq!(
                value["rows"][primary.turn.to_string()]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        assert_eq!(streaming["inputs"], committed["inputs"]);
        assert_eq!(committed["inputs"], reconnected["inputs"]);
    }
}

#[test]
fn keeps_queued_turns_and_unloaded_owners_distinct() {
    let mut page = fixture();
    let primary = page.runs[0].clone();
    let guide = joined(&primary, 2, "Delivered guidance");
    let queued = Run {
        turn: TurnId::new(),
        sequence: 3,
        status: Status::Queued,
        started_ms: None,
        guidance: Some(primary.turn),
        input: Some(SentInput {
            message: "A separate task".into(),
            attachments: vec![],
        }),
        ..primary.clone()
    };
    page.runs.extend([guide.clone(), queued.clone()]);
    page.queue.items.push(Pending {
        turn: queued.turn,
        revision: 1,
        config_revision: 2,
        ready: true,
        guidance: None,
        notice: false,
        request: None,
        kind: RunKind::Task,
        preview: "A separate task".into(),
        truncated: false,
        attachments: vec![],
    });
    Arc::make_mut(&mut page.entries).push(entry(
        guide.turn,
        2,
        "user",
        vec![Part::Text("Delivered guidance".into())],
    ));
    let value = encoded(&page, vec![]);
    assert_eq!(value["timeline"], json!([primary.turn, queued.turn]));
    assert_eq!(
        value["inputs"][primary.turn.to_string()]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        value["inputs"][queued.turn.to_string()]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        value["inputs"][queued.turn.to_string()][0]["turn"],
        json!(queued.turn)
    );
    assert_eq!(
        value["inputs"][queued.turn.to_string()][0]["canonical"],
        false
    );
    page.runs.retain(|run| run.turn != primary.turn);
    let value = encoded(&page, vec![]);
    assert_eq!(value["timeline"], json!([guide.turn, queued.turn]));
    assert_eq!(value["owners"][guide.turn.to_string()], json!(guide.turn));
    assert_eq!(
        value["inputs"][guide.turn.to_string()][0]["turn"],
        json!(guide.turn)
    );
    assert_eq!(
        value["inputs"][guide.turn.to_string()][0]["message"]["text"],
        "Delivered guidance"
    );
}
