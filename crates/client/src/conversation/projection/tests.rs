use super::*;
use sailry_protocol::conversation::{Entry, Frame, Run};

mod approvals;
mod children;
mod forks;
mod history;
mod questions;
mod queue;
mod statistics;

fn fixture() -> (Projection, Snapshot, Run, Draft) {
    let node = NodeId([61; 32]);
    let session = SessionId::new();
    let run = Run {
        worktree: sailry_protocol::WorktreeId::new(),
        kind: sailry_protocol::conversation::RunKind::Task,
        turn: TurnId::new(),
        session,
        sequence: 1,
        revision: 3,
        status: Status::Running,
        error: None,
        started_ms: Some(1000),
        finished_ms: None,
        origin: None,
    };
    let snapshot = Snapshot {
        node,
        sequence: 10,
        missing: Vec::new(),
        page: Arc::new(Page {
            session,
            revision: 1,
            entries: Vec::new(),
            runs: vec![run.clone()],
            next_before: None,
            queue: Default::default(),
            approvals: Vec::new(),
            questions: Vec::new(),
            children: Vec::new(),
        }),
        drafts: Vec::new(),
        statistics: Default::default(),
    };
    let draft = Draft {
        id: "event-fixture".into(),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Text("Hello ".into())],
    };
    (Projection::new(node, session, 1), snapshot, run, draft)
}

fn frame(snapshot: &Snapshot, sequence: u64, change: Change) -> Update {
    Update::ConversationFrame(Frame {
        node: snapshot.node,
        session: snapshot.page.session,
        sequence,
        change,
    })
}

#[test]
fn rejects_changed_run_kind() {
    let (mut projection, snapshot, mut run, _) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    run.kind = sailry_protocol::conversation::RunKind::Compaction;
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Run(run)))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap().page, snapshot.page);
}

#[test]
fn commits_canonical_output() {
    let (mut projection, snapshot, mut run, mut draft) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let update = frame(&snapshot, 11, Change::Delta(draft.clone()));
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    draft.parts = vec![Part::Text("中文🙂".into())];
    projection
        .apply(1, frame(&snapshot, 12, Change::Delta(draft.clone())))
        .unwrap();
    let view = projection.snapshot().unwrap();
    assert!(view.page.entries.is_empty());
    assert_eq!(
        view.drafts[0].parts,
        vec![Part::Text("Hello 中文🙂".into())]
    );
    assert!(Arc::ptr_eq(&snapshot.page, &view.page));
    let entry = Entry {
        sequence: 41,
        id: draft.id,
        turn: draft.turn,
        author: draft.author,
        branch: draft.branch,
        timestamp_ms: 1,
        parts: vec![Part::Text("committed content".into())],
        citations: vec![],
        search_suggestions: None,
        usage: None,
    };
    projection
        .apply(1, frame(&snapshot, 13, Change::Entry(entry.clone())))
        .unwrap();
    assert!(projection.snapshot().unwrap().drafts.is_empty());
    assert_eq!(projection.snapshot().unwrap().page.entries, vec![entry]);
    run.status = Status::Completed;
    projection
        .apply(1, frame(&snapshot, 14, Change::Run(run.clone())))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().page.runs, vec![run]);
}

#[test]
fn recovers_restarted_sequences() {
    let (mut projection, mut snapshot, _, draft) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 12, Change::Delta(draft.clone())))
            .unwrap(),
        Apply::Recover
    );
    assert!(projection.snapshot().unwrap().drafts.is_empty());
    projection.reconnect(2).unwrap();
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Delta(draft.clone())))
            .unwrap(),
        Apply::Ignored
    );
    snapshot.sequence = 1;
    snapshot.drafts.push(draft);
    assert_eq!(
        projection
            .apply(2, Update::ConversationSnapshot(snapshot.clone()))
            .unwrap(),
        Apply::Applied
    );
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
}

#[test]
fn validates_frames_and_cancellation() {
    let (mut projection, snapshot, mut run, draft) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut other = snapshot.clone();
    other.node = NodeId([62; 32]);
    assert_eq!(
        projection
            .apply(1, Update::ConversationSnapshot(other))
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    projection
        .apply(1, frame(&snapshot, 11, Change::Delta(draft.clone())))
        .unwrap();
    run.status = Status::Cancelled;
    projection
        .apply(1, frame(&snapshot, 12, Change::Run(run)))
        .unwrap();
    assert!(projection.snapshot().unwrap().drafts.is_empty());
    assert!(
        projection
            .apply(1, frame(&snapshot, 13, Change::Delta(draft)))
            .is_err()
    );
}
