use super::*;

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
    };
    let mut feeds = Feeds::default();
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
    };
    let closed = CancellationToken::new();
    let history = History {
        sequence: 0,
        page,
        missing: Vec::new(),
    };
    let mut subscription =
        feeds.subscribe(node, history.clone(), Default::default(), closed.clone());
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
    let mut resumed = feeds.subscribe(node, history, Default::default(), closed.clone());
    let Update::ConversationSnapshot(snapshot) = resumed.next().await.unwrap() else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.drafts[0].parts, vec![Part::Text("x".repeat(100))]);
    closed.cancel();
    assert!(resumed.next().await.is_err());
    drop(resumed);
    run.status = Status::Cancelled;
    feeds.publish(node, session, Change::Run(run));
    feeds.prune();
    assert!(feeds.0.is_empty());
}
