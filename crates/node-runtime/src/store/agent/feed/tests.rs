use super::*;

mod transport;

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
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "normalizes_first_delta",
            (|scenario: &'static str| {
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
                    assert_eq!(feed.drafts[0].parts, expected, "{scenario}");
                    assert_eq!(
                        feed.bytes,
                        expected.iter().map(part_bytes).sum::<usize>(),
                        "{scenario}"
                    );
                }
                feeds.publish(node, session, Change::Queue(Default::default()));
                let feed = &feeds.0.lock().unwrap()[&session];
                assert_eq!(
                    feed.bytes,
                    expected.iter().map(part_bytes).sum::<usize>(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "counts_merged_payload_once",
            (|scenario: &'static str| {
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
                    ],
                    "{scenario}"
                );
                // Escaped payload, two Part envelopes, and no per-token envelope charge.
                assert_eq!(
                    feed.bytes,
                    70_000 + 10 + 9 + 6 + 2 * PART_OVERHEAD,
                    "{scenario}"
                );
            }),
        ),
        (
            "rejects_overflow_without_losing_drafts",
            (|scenario: &'static str| {
                let (feeds, node, session, mut draft) = streaming();
                draft.parts = vec![Part::Text("x".repeat(2 * 1024 * 1024 - PART_OVERHEAD))];
                feeds.delta(node, session, draft.clone()).unwrap();
                let sequence = feeds.sequence(session);
                draft.parts = vec![Part::Text("x".into())];
                assert!(
                    feeds.delta(node, session, draft.clone()).is_err(),
                    "{scenario}"
                );
                draft.parts = vec![Part::Thinking(String::new())];
                assert!(feeds.delta(node, session, draft).is_err(), "{scenario}");
                assert_eq!(feeds.sequence(session), sequence, "{scenario}");
                let feed = &feeds.0.lock().unwrap()[&session];
                assert_eq!(feed.bytes, 2 * 1024 * 1024, "{scenario}");
                assert_eq!(feed.drafts[0].parts.len(), 1, "{scenario}");
                assert!(
                    matches!(&feed.drafts[0].parts[0],Part::Text(text) if text.len() == 2 * 1024 * 1024 - PART_OVERHEAD),
                    "{scenario}"
                );

                let (feeds, node, session, mut draft) = streaming();
                draft.parts = vec![Part::Text("\n".repeat(1024 * 1024))];
                assert!(feeds.delta(node, session, draft).is_err(), "{scenario}");
                assert!(
                    feeds.0.lock().unwrap()[&session].drafts.is_empty(),
                    "{scenario}"
                );
            }),
        ),
        (
            "bounds_retained_bytes",
            (|scenario: &'static str| {
                let (feeds, node, session, mut draft) = streaming();
                draft.parts = vec![Part::Text("x".repeat(1024 * 1024))];
                for _ in 0..8 {
                    let mut retained = feeds.0.lock().unwrap();
                    let feed = retained.get_mut(&session).unwrap();
                    feed.send(node, session, Change::Delta(draft.clone()));
                    assert!(feed.retained_bytes <= MAX_RETAINED_BYTES, "{scenario}");
                    assert_eq!(
                        feed.retained_bytes,
                        feed.retained.iter().map(|(_, bytes)| bytes).sum::<usize>(),
                        "{scenario}"
                    );
                }
                assert!(
                    feeds.0.lock().unwrap()[&session].retained.len() < 8,
                    "{scenario}"
                );
            }),
        ),
        (
            "seals_stopping_before_cancellation",
            (|scenario: &'static str| {
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
                    guidance: None,
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
                assert!(!stop.is_cancelled(), "{scenario}");
                draft.id = "after-stop".into();
                draft.parts = vec![Part::Text("late".into())];
                controls
                    .stream(run.turn, session, || {
                        feeds.delta(node, session, draft.clone())
                    })
                    .unwrap();
                assert_eq!(feeds.sequence(session), stopping, "{scenario}");
                assert_eq!(
                    feeds.0.lock().unwrap()[&session].drafts.len(),
                    1,
                    "{scenario}"
                );
                assert_eq!(
                    feeds.0.lock().unwrap()[&session].drafts[0].parts,
                    vec![Part::Text("before stop".into())],
                    "{scenario}"
                );

                run.status = Status::Cancelled;
                feeds.publish(node, session, Change::Run(run.clone()));
                let terminal = feeds.sequence(session);
                controls
                    .stream(run.turn, session, || feeds.delta(node, session, draft))
                    .unwrap();
                assert_eq!(feeds.sequence(session), terminal, "{scenario}");
                assert!(
                    feeds.0.lock().unwrap()[&session].drafts.is_empty(),
                    "{scenario}"
                );
            }),
        ),
        (
            "bounds_finalized_seals_without_eviction",
            (|scenario: &'static str| {
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
                assert!(
                    feeds.0.lock().unwrap()[&session].finalized.is_none(),
                    "{scenario}"
                );
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
                assert_eq!(feeds.sequence(session), sequence, "{scenario}");
                assert!(
                    feeds.0.lock().unwrap()[&session].drafts.is_empty(),
                    "{scenario}"
                );
            }),
        ),
        (
            "skips_unchanged_background_tasks",
            (|scenario: &'static str| {
                use sailry_protocol::conversation::background::{Kind, State, Task};
                let (feeds, node, session, draft) = streaming();
                let mut task = Task {
                    id: "command:fixture".into(),
                    session,
                    turn: draft.turn,
                    kind: Kind::Command {
                        id: sailry_protocol::RequestId::new(),
                    },
                    title: "Build 中文".into(),
                    revision: 1,
                    state: State::Running,
                    outcome: None,
                    started_ms: 1,
                    finished_ms: None,
                    summary: None,
                    delivery: None,
                    recovered: false,
                    suppressed: false,
                };
                let start = feeds.sequence(session);
                for _ in 0..3 {
                    feeds.publish(node, session, Change::Background(task.clone()));
                }
                assert_eq!(feeds.sequence(session), start + 1, "{scenario}");
                task.revision = 2;
                feeds.publish(node, session, Change::Background(task.clone()));
                assert_eq!(feeds.sequence(session), start + 2, "{scenario}");
                // A reset starts a new generation that republishes current task state.
                feeds.reset(session);
                let reset = feeds.sequence(session);
                feeds.publish(node, session, Change::Background(task));
                assert_eq!(feeds.sequence(session), reset + 1, "{scenario}");
            }),
        ),
        (
            "hidden_entry_supersedes_draft",
            (|scenario: &'static str| {
                let (feeds, node, session, draft) = streaming();
                feeds.delta(node, session, draft.clone()).unwrap();
                feeds.publish(
                    node,
                    session,
                    Change::Hidden {
                        turn: draft.turn,
                        id: draft.id.clone(),
                    },
                );
                assert!(
                    feeds.0.lock().unwrap()[&session].drafts.is_empty(),
                    "{scenario}"
                );
                // Late chunks of a committed entry are no longer streamed.
                let sequence = feeds.sequence(session);
                feeds.delta(node, session, draft).unwrap();
                assert_eq!(feeds.sequence(session), sequence, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[tokio::test]
async fn replays_retained_sequences() {
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
        guidance: None,
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
        entries: Vec::new().into(),
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
    for sequence in 2..=101 {
        let Update::ConversationFrame(frame) = subscription.next().await.unwrap() else {
            panic!("retained frame expected")
        };
        assert_eq!(frame.sequence, sequence);
        assert!(
            matches!(frame.change, Change::Delta(draft) if draft.parts == [Part::Text("x".into())])
        );
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), subscription.next())
            .await
            .is_err()
    );
    feeds.publish(node, session, Change::Queue(Default::default()));
    let Update::ConversationFrame(frame) = subscription.next().await.unwrap() else {
        panic!("live frame expected")
    };
    assert_eq!(frame.sequence, 102);
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
    for _ in 0..=MAX_RETAINED {
        feeds.publish(node, session, Change::Queue(Default::default()));
    }
    assert_eq!(resumed.next().await.unwrap(), Update::ResetRequired);
    closed.cancel();
    assert!(resumed.next().await.is_err());
    drop(resumed);
    run.status = Status::Cancelled;
    feeds.publish(node, session, Change::Run(run));
    Feeds::prune(&mut feeds.0.lock().unwrap());
    assert!(feeds.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reset_invalidates_retained_gap() {
    let (feeds, node, session, draft) = streaming();
    let reserved = feeds.reserve(session);
    feeds.delta(node, session, draft.clone()).unwrap();
    feeds.reset(session);
    for _ in 0..100 {
        feeds.delta(node, session, draft.clone()).unwrap();
    }
    let mut updates = Updates {
        initial: None,
        feeds,
        session,
        sequence: reserved.sequence,
        pending: VecDeque::new(),
        updates: reserved.updates,
        closed: CancellationToken::new(),
    };
    assert_eq!(updates.next().await.unwrap(), Update::ResetRequired);
}

#[tokio::test]
async fn coalesces_batch_statistics_per_session() {
    let mut fixture = crate::store::agent::tests::fixture::Fixture::new().await;
    let first = fixture.queued().await;
    let second = fixture.queued().await;
    let runs = vec![
        runs::get(&fixture.database.connection, first)
            .await
            .unwrap(),
        runs::get(&fixture.database.connection, second)
            .await
            .unwrap(),
    ];
    let queue = super::super::queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let feed = Feed::default();
    let _subscription = feed.updates.subscribe();
    fixture
        .database
        .feeds
        .0
        .lock()
        .unwrap()
        .insert(fixture.session, feed);
    command(
        &mut fixture.database,
        &Output::Queue(sailry_protocol::conversation::QueueUpdate {
            session: fixture.session,
            queue,
            runs,
        }),
    )
    .await;
    let feeds = fixture.database.feeds.0.lock().unwrap();
    let changes = feeds[&fixture.session]
        .retained
        .iter()
        .map(|(frame, _)| &frame.change)
        .collect::<Vec<_>>();
    assert_eq!(
        changes
            .iter()
            .filter(|change| matches!(change, Change::Run(_)))
            .count(),
        2
    );
    assert_eq!(
        changes
            .iter()
            .filter(|change| matches!(change, Change::Statistics(_)))
            .count(),
        1
    );
    assert!(matches!(changes.last(), Some(Change::Queue(_))));
}
