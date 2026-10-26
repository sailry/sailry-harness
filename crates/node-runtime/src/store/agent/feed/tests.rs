use super::*;

fn streaming() -> (Feeds, NodeId, SessionId, Draft) {
    let node = NodeId([74; 32]);
    let session = SessionId::new();
    let turn = TurnId::new();
    let feeds = Feeds::default();
    feeds.0.lock().unwrap().insert(
        session,
        Feed {
            active: Some(turn),
            ..Default::default()
        },
    );
    let draft = Draft {
        id: "answer".into(),
        turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Text("x".into())],
    };
    (feeds, node, session, draft)
}

#[test]
fn normalizes_first_delta() {
    let (feeds, node, session, mut draft) = streaming();
    draft.parts = vec![
        Part::Text("A".into()),
        Part::Text("\\\n".into()),
        Part::Thinking("B".into()),
        Part::Thinking("C".into()),
        Part::Text("D".into()),
    ];
    feeds.delta(node, session, draft).unwrap();
    let expected = [
        Part::Text("A\\\n".into()),
        Part::Thinking("BC".into()),
        Part::Text("D".into()),
    ];
    {
        let feed = &feeds.0.lock().unwrap()[&session];
        assert_eq!(feed.drafts[0].parts, expected);
        assert_eq!(feed.bytes, expected.iter().map(part_bytes).sum::<usize>());
    }
    feeds.publish(node, session, Change::Queue(Default::default()));
    let feed = &feeds.0.lock().unwrap()[&session];
    assert_eq!(feed.bytes, expected.iter().map(part_bytes).sum::<usize>());
}

#[test]
fn counts_merged_payload_once() {
    let (feeds, node, session, mut draft) = streaming();
    for _ in 0..70_000 {
        feeds.delta(node, session, draft.clone()).unwrap();
    }
    draft.parts = vec![
        Part::Text("\n\"\\🦀".into()),
        Part::Thinking("Reasoning".into()),
        Part::Thinking("\nMore".into()),
    ];
    feeds.delta(node, session, draft).unwrap();
    let feeds = feeds.0.lock().unwrap();
    let feed = &feeds[&session];
    assert_eq!(
        feed.drafts[0].parts,
        [
            Part::Text(format!("{}\n\"\\🦀", "x".repeat(70_000))),
            Part::Thinking("Reasoning\nMore".into())
        ]
    );
    // Escaped payload, two Part envelopes, and no per-token envelope charge.
    assert_eq!(feed.bytes, 70_000 + 10 + 9 + 6 + 2 * PART_OVERHEAD);
}

#[test]
fn rejects_overflow_without_losing_drafts() {
    let (feeds, node, session, mut draft) = streaming();
    draft.parts = vec![Part::Text("x".repeat(2 * 1024 * 1024 - PART_OVERHEAD))];
    feeds.delta(node, session, draft.clone()).unwrap();
    let sequence = feeds.sequence(session);
    draft.parts = vec![Part::Text("x".into())];
    assert!(feeds.delta(node, session, draft.clone()).is_err());
    draft.parts = vec![Part::Thinking(String::new())];
    assert!(feeds.delta(node, session, draft).is_err());
    assert_eq!(feeds.sequence(session), sequence);
    let feed = &feeds.0.lock().unwrap()[&session];
    assert_eq!(feed.bytes, 2 * 1024 * 1024);
    assert_eq!(feed.drafts[0].parts.len(), 1);
    assert!(
        matches!(&feed.drafts[0].parts[0],Part::Text(text) if text.len() == 2 * 1024 * 1024 - PART_OVERHEAD)
    );

    let (feeds, node, session, mut draft) = streaming();
    draft.parts = vec![Part::Text("\n".repeat(1024 * 1024))];
    assert!(feeds.delta(node, session, draft).is_err());
    assert!(feeds.0.lock().unwrap()[&session].drafts.is_empty());
}

#[tokio::test]
async fn recovers_overflow() {
    let node = NodeId([71; 32]);
    let session = SessionId::new();
    let mut run = Run {
        worktree: sailry_protocol::WorktreeId::new(),
        kind: sailry_protocol::conversation::RunKind::Task,
        turn: TurnId::new(),
        session,
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1000),
        finished_ms: None,
        origin: None,
        continuation: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    };
    let feeds = Feeds::default();
    feeds.publish(node, session, Change::Run(run.clone()));
    let page = Page {
        session,
        revision: 1,
        entries: Vec::new(),
        runs: vec![run.clone()],
        next_before: None,
        queue: Default::default(),
        approvals: Vec::new(),
        questions: Vec::new(),
        children: Vec::new(),
        background: Vec::new(),
    };
    let closed = CancellationToken::new();
    let history = History {
        sequence: 0,
        page,
        missing: Vec::new(),
    };
    let mut subscription = feeds
        .subscribe(
            node,
            history.clone(),
            Default::default(),
            Default::default(),
            closed.clone(),
        )
        .unwrap();
    assert!(matches!(
        subscription.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    for _ in 0..100 {
        feeds
            .delta(
                node,
                session,
                Draft {
                    id: "overflow-fixture".into(),
                    turn: run.turn,
                    author: "assistant".into(),
                    branch: String::new(),
                    parts: vec![Part::Text("x".into())],
                },
            )
            .unwrap();
    }
    assert_eq!(subscription.next().await.unwrap(), Update::ResetRequired);
    drop(subscription);
    let mut resumed = feeds
        .subscribe(
            node,
            history,
            Default::default(),
            Default::default(),
            closed.clone(),
        )
        .unwrap();
    let Update::ConversationSnapshot(snapshot) = resumed.next().await.unwrap() else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.drafts[0].parts, vec![Part::Text("x".repeat(100))]);
    closed.cancel();
    assert!(resumed.next().await.is_err());
    drop(resumed);
    run.status = Status::Cancelled;
    feeds.publish(node, session, Change::Run(run));
    Feeds::prune(&mut feeds.0.lock().unwrap());
    assert!(feeds.0.lock().unwrap().is_empty());
}

#[test]
fn seals_stopping_before_cancellation() {
    let node = NodeId([73; 32]);
    let session = SessionId::new();
    let mut run = Run {
        worktree: sailry_protocol::WorktreeId::new(),
        kind: sailry_protocol::conversation::RunKind::Task,
        turn: TurnId::new(),
        session,
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1000),
        finished_ms: None,
        origin: None,
        continuation: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    };
    let controls = crate::agent::Controls::default();
    let stop = CancellationToken::new();
    controls.register(run.turn, session, stop.clone());
    let feeds = Feeds::default();
    feeds.publish(node, session, Change::Run(run.clone()));
    let mut draft = Draft {
        id: "answer".into(),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        parts: vec![Part::Text("before stop".into())],
    };
    feeds.delta(node, session, draft.clone()).unwrap();
    run.status = Status::Stopping;
    feeds.publish(node, session, Change::Run(run.clone()));
    let stopping = feeds.sequence(session);
    assert!(!stop.is_cancelled());
    draft.id = "after-stop".into();
    draft.parts = vec![Part::Text("late".into())];
    controls
        .stream(run.turn, session, || {
            feeds.delta(node, session, draft.clone())
        })
        .unwrap();
    assert_eq!(feeds.sequence(session), stopping);
    assert_eq!(feeds.0.lock().unwrap()[&session].drafts.len(), 1);
    assert_eq!(
        feeds.0.lock().unwrap()[&session].drafts[0].parts,
        vec![Part::Text("before stop".into())]
    );

    run.status = Status::Cancelled;
    feeds.publish(node, session, Change::Run(run.clone()));
    let terminal = feeds.sequence(session);
    controls
        .stream(run.turn, session, || feeds.delta(node, session, draft))
        .unwrap();
    assert_eq!(feeds.sequence(session), terminal);
    assert!(feeds.0.lock().unwrap()[&session].drafts.is_empty());
}

#[test]
fn bounds_finalized_seals_without_eviction() {
    let node = NodeId([72; 32]);
    let session = SessionId::new();
    let turn = TurnId::new();
    let feeds = Feeds::default();
    feeds.0.lock().unwrap().insert(
        session,
        Feed {
            active: Some(turn),
            ..Default::default()
        },
    );
    for index in 0..=MAX_FINALIZED {
        feeds.publish(
            node,
            session,
            Change::Entry(Entry {
                sequence: index as u64,
                id: format!("event-{index}"),
                turn,
                author: "assistant".into(),
                branch: String::new(),
                timestamp_ms: 0,
                parts: Vec::new(),
                citations: Vec::new(),
                search_suggestions: None,
                usage: None,
            }),
        );
    }
    let sequence = feeds.sequence(session);
    assert!(feeds.0.lock().unwrap()[&session].finalized.is_none());
    for id in ["event-0", "new-event"] {
        feeds
            .delta(
                node,
                session,
                Draft {
                    id: id.into(),
                    turn,
                    author: "assistant".into(),
                    branch: String::new(),
                    parts: vec![Part::Text("late".into())],
                },
            )
            .unwrap();
    }
    assert_eq!(feeds.sequence(session), sequence);
    assert!(feeds.0.lock().unwrap()[&session].drafts.is_empty());
}
