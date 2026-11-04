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
fn waits_for_canonical_input_after_claim() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.runs[0].joined = page.runs[0].guidance;
    page.runs[0].status = Status::Continued;
    assert_eq!(project(&page)[&turn], State::Pending);
    page.queue.items.clear();
    assert_eq!(project(&page)[&turn], State::Pending);
    append(&mut page, "assistant");
    assert_eq!(project(&page)[&turn], State::Pending);
    append(&mut page, "user");
    assert_eq!(project(&page)[&turn], State::Delivered);
}

#[test]
fn canonical_input_can_precede_claim() {
    let mut page = page();
    let turn = page.runs[0].turn;
    append(&mut page, "user");
    assert_eq!(project(&page)[&turn], State::Pending);
    page.runs[0].joined = page.runs[0].guidance;
    assert_eq!(project(&page)[&turn], State::Delivered);
}

#[test]
fn canonical_delivery_survives_stale_queue() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.runs[0].joined = page.runs[0].guidance;
    append(&mut page, "user");
    page.queue.items[0].guidance = None;
    assert_eq!(project(&page)[&turn], State::Delivered);
}

#[test]
fn restored_input_becomes_next_turn() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.queue.items[0].guidance = None;
    assert_eq!(project(&page)[&turn], State::NextTurn);
    page.queue.items.clear();
    page.runs[0].status = Status::Running;
    assert_eq!(project(&page)[&turn], State::NextTurn);
    append(&mut page, "user");
    assert_eq!(project(&page)[&turn], State::NextTurn);
}

#[test]
fn successor_keeps_original_target() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.queue.items.clear();
    page.runs[0].status = Status::Continued;
    page.runs[0].joined = Some(TurnId::new());
    append(&mut page, "user");
    assert_eq!(project(&page)[&turn], State::NextTurn);
}

#[test]
fn queue_can_precede_run() {
    let mut page = page();
    let turn = page.runs[0].turn;
    page.runs.clear();
    assert_eq!(project(&page)[&turn], State::Pending);
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

#[test]
fn reselected_recipient_reports_delivery() {
    let mut page = page();
    let turn = page.runs[0].turn;
    // Explicit reselection durably replaces the original target.
    let recipient = TurnId::new();
    page.runs[0].guidance = Some(recipient);
    page.queue.items.clear();
    page.runs[0].status = Status::Continued;
    page.runs[0].joined = Some(recipient);
    append(&mut page, "user");
    assert_eq!(project(&page)[&turn], State::Delivered);
}
