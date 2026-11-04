use super::*;
use sailry_protocol::{
    NodeId, SessionId, WorktreeId,
    conversation::{Entry, Page, Part, Pending, Queue, Run, RunKind, Snapshot, Status},
};
use std::sync::Arc;

fn page() -> Page {
    let recipient = TurnId::new();
    let run = Run {
        worktree: WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 2,
        revision: 4,
        status: Status::Queued,
        error: None,
        started_ms: None,
        finished_ms: None,
        origin: None,
        continuation: None,
        guidance: Some(recipient),
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    };
    Page {
        session: run.session,
        revision: 1,
        entries: Vec::new().into(),
        queue: Queue {
            items: vec![Pending {
                turn: run.turn,
                revision: 1,
                config_revision: 4,
                ready: true,
                guidance: Some(recipient),
                notice: false,
                request: None,
                kind: RunKind::Task,
                preview: "Focus on the failure".into(),
                truncated: false,
                attachments: Vec::new(),
            }],
            ..Default::default()
        },
        runs: vec![run],
        approvals: Vec::new(),
        questions: Vec::new(),
        children: Vec::new(),
        background: Vec::new(),
        next_before: None,
    }
}

fn append(page: &mut Page, author: &str) {
    let mut entries: Vec<_> = page.entries.iter().cloned().collect();
    entries.push(Entry {
        sequence: 10 + entries.len() as u64,
        id: format!("guidance-input-{}", entries.len()),
        turn: page.runs[0].turn,
        author: author.into(),
        branch: String::new(),
        parts: vec![Part::Text("Focus on the failure".into())],
        timestamp_ms: 1000,
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    });
    page.entries = entries.into();
}

#[test]
fn follows_canonical_delivery_transitions() {
    type Step = (fn(&mut Page), State);
    let cases: &[(&str, &[Step])] = &[
        (
            "claim before input",
            &[
                (
                    |page| {
                        page.runs[0].joined = page.runs[0].guidance;
                        page.runs[0].status = Status::Continued;
                    },
                    State::Pending,
                ),
                (|page| page.queue.items.clear(), State::Pending),
                (|page| append(page, "assistant"), State::Pending),
                (|page| append(page, "user"), State::Delivered),
            ],
        ),
        (
            "input before claim",
            &[
                (|page| append(page, "user"), State::Pending),
                (
                    |page| page.runs[0].joined = page.runs[0].guidance,
                    State::Delivered,
                ),
            ],
        ),
        (
            "stale queue",
            &[(
                |page| {
                    page.runs[0].joined = page.runs[0].guidance;
                    append(page, "user");
                    page.queue.items[0].guidance = None;
                },
                State::Delivered,
            )],
        ),
        (
            "restored next turn",
            &[
                (|page| page.queue.items[0].guidance = None, State::NextTurn),
                (
                    |page| {
                        page.queue.items.clear();
                        page.runs[0].status = Status::Running;
                    },
                    State::NextTurn,
                ),
                (|page| append(page, "user"), State::NextTurn),
            ],
        ),
        (
            "successor retains target",
            &[(
                |page| {
                    page.queue.items.clear();
                    page.runs[0].status = Status::Continued;
                    page.runs[0].joined = Some(TurnId::new());
                    append(page, "user");
                },
                State::NextTurn,
            )],
        ),
        (
            "queue before run",
            &[(|page| page.runs.clear(), State::Pending)],
        ),
        (
            "reselected recipient",
            &[(
                |page| {
                    // Explicit reselection durably replaces the original target.
                    let recipient = TurnId::new();
                    page.runs[0].guidance = Some(recipient);
                    page.queue.items.clear();
                    page.runs[0].status = Status::Continued;
                    page.runs[0].joined = Some(recipient);
                    append(page, "user");
                },
                State::Delivered,
            )],
        ),
    ];
    for (case, steps) in cases {
        let mut page = page();
        let turn = page.runs[0].turn;
        for (index, (apply, expected)) in steps.iter().enumerate() {
            apply(&mut page);
            assert_eq!(project(&page)[&turn], *expected, "{case}, step {index}");
        }
    }
}

#[test]
fn ordinary_joined_inputs_have_no_guidance_state() {
    let mut page = page();
    page.runs[0].guidance = None;
    page.runs[0].joined = Some(TurnId::new());
    page.queue.items[0].guidance = None;
    append(&mut page, "user");
    assert!(project(&page).is_empty());
}

#[test]
fn observer_replacement_refreshes_shared_state() {
    let page = page();
    let turn = page.runs[0].turn;
    let mut snapshot = Snapshot {
        node: NodeId([7; 32]),
        sequence: 1,
        page: Arc::new(page),
        missing: Vec::new(),
        drafts: Vec::new(),
        statistics: None,
        artifacts: Default::default(),
    };
    let mut view = crate::conversation::View::default();
    view.replace(Arc::new(snapshot.clone()), 0, 1, None);
    assert_eq!(view.guidance[&turn], State::Pending);
    let pending = view.guidance.clone();
    snapshot.sequence += 1;
    view.replace(Arc::new(snapshot.clone()), 0, 1, None);
    assert!(Arc::ptr_eq(&pending, &view.guidance));
    let page = Arc::make_mut(&mut snapshot.page);
    page.queue.items.clear();
    page.runs[0].joined = page.runs[0].guidance;
    append(page, "user");
    view.replace(Arc::new(snapshot), 0, 2, None);
    assert_eq!(view.guidance[&turn], State::Delivered);
    assert_eq!(
        serde_json::to_value(&view).unwrap()["guidance"][turn.to_string()],
        "delivered"
    );
}
