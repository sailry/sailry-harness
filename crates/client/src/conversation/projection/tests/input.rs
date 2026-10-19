use super::*;
use sailry_protocol::conversation::{Input, RunKind, SentInput};

#[test]
fn canonical_user_replaces_capture() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    let attachment = sailry_protocol::attachment::Attachment {
        id: sailry_protocol::AttachmentId::new(),
        spec: sailry_protocol::attachment::Spec {
            worktree: run.worktree,
            name: "captured.png".into(),
            media_type: "image/png".into(),
            size: 64,
            revision: "fixture-image-digest".into(),
        },
    };
    let reference = sailry_protocol::conversation::reference::Reference {
        target: sailry_protocol::conversation::reference::Target::Project,
        label: "Captured project".into(),
    };
    let captured = Input {
        text: "Original sent input".into(),
        attachments: vec![attachment.id],
        references: vec![reference.clone()],
    };
    run.input = Some(SentInput {
        message: captured.clone(),
        attachments: vec![attachment.clone()],
    });
    Arc::make_mut(&mut snapshot.page).runs[0] = run.clone();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let input =
        super::super::super::turns::input(&projection.snapshot().unwrap().page, run.turn).unwrap();
    assert_eq!(input.message, captured);
    assert_eq!(
        input.attachments.as_slice(),
        std::slice::from_ref(&attachment)
    );
    let entry = Entry {
        sequence: 1,
        id: "canonical-input".into(),
        turn: run.turn,
        author: "user".into(),
        branch: String::new(),
        timestamp_ms: 0,
        parts: vec![
            Part::Text("Canonical input".into()),
            Part::Attachment(attachment),
            Part::Reference(reference),
        ],
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    };
    projection
        .apply(1, frame(&snapshot, 11, Change::Entry(entry.clone())))
        .unwrap();
    let current = projection.snapshot().unwrap();
    assert!(current.page.runs[0].input.is_none());
    assert_eq!(current.page.entries, [entry]);
    assert_eq!(
        super::super::super::turns::input(&current.page, run.turn)
            .unwrap()
            .message
            .text,
        "Canonical input"
    );
    let canonical = super::super::super::turns::input(&current.page, run.turn).unwrap();
    assert_eq!(canonical.message.attachments, captured.attachments);
    assert_eq!(canonical.message.references, captured.references);
    assert_eq!(canonical.attachments, input.attachments);
}

#[test]
fn rejects_changed_capture_and_private_runs() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.input = Some(SentInput {
        message: "Immutable input".into(),
        attachments: Vec::new(),
    });
    Arc::make_mut(&mut snapshot.page).runs[0] = run.clone();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let original = projection.snapshot().unwrap().clone();
    run.input.as_mut().unwrap().message.text = "Changed input".into();
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Run(run)))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &original);
    let mut invalid = snapshot.clone();
    Arc::make_mut(&mut invalid.page).runs[0].kind = RunKind::Compaction;
    let mut fresh = Projection::new(snapshot.node, snapshot.page.session, 1);
    assert!(
        fresh
            .apply(1, Update::ConversationSnapshot(invalid))
            .is_err()
    );
    let mut invalid = snapshot.clone();
    Arc::make_mut(&mut invalid.page).runs[0].status = Status::Queued;
    Arc::make_mut(&mut invalid.page).runs[0].notice = true;
    let mut fresh = Projection::new(snapshot.node, snapshot.page.session, 1);
    assert!(
        fresh
            .apply(1, Update::ConversationSnapshot(invalid))
            .is_err()
    );
}

#[test]
fn invalid_user_event_preserves_capture() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.input = Some(SentInput {
        message: "Preserve this input".into(),
        attachments: Vec::new(),
    });
    Arc::make_mut(&mut snapshot.page).runs[0] = run.clone();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let entry = Entry {
        sequence: 0,
        id: "invalid-user".into(),
        turn: run.turn,
        author: "user".into(),
        branch: String::new(),
        timestamp_ms: 0,
        parts: vec![Part::Text("Rejected event".into())],
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    };
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Entry(entry)))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
}

#[test]
fn reconnect_restores_capture_with_same_turn() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.status = Status::Failed;
    run.input = Some(SentInput {
        message: "Failed input 中文 🙂".into(),
        attachments: Vec::new(),
    });
    Arc::make_mut(&mut snapshot.page).runs[0] = run.clone();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    projection.reconnect(2).unwrap();
    snapshot.sequence = 1;
    projection
        .apply(2, Update::ConversationSnapshot(snapshot))
        .unwrap();
    let current = projection.snapshot().unwrap();
    assert!(current.page.entries.is_empty());
    assert_eq!(current.page.runs, [run.clone()]);
    assert_eq!(
        super::super::super::turns::input(&current.page, run.turn),
        run.input
    );
}

#[test]
fn preserves_queued_child_input_through_claim() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.status = Status::Queued;
    run.input = Some(SentInput {
        message: "Independent child task 中文 🙂".into(),
        attachments: Vec::new(),
    });
    Arc::make_mut(&mut snapshot.page).runs[0] = run.clone();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let original =
        super::super::super::turns::input(&projection.snapshot().unwrap().page, run.turn).unwrap();
    run.status = Status::Running;
    projection
        .apply(1, frame(&snapshot, 11, Change::Run(run.clone())))
        .unwrap();
    assert_eq!(
        super::super::super::turns::input(&projection.snapshot().unwrap().page, run.turn),
        Some(original.clone())
    );
    let mut changed = run;
    changed.input.as_mut().unwrap().message.text =
        "Parent prompt must not replace child input".into();
    assert!(
        projection
            .apply(1, frame(&snapshot, 12, Change::Run(changed)))
            .is_err()
    );
    assert_eq!(
        super::super::super::turns::input(
            &projection.snapshot().unwrap().page,
            snapshot.page.runs[0].turn
        ),
        Some(original)
    );
}
