use super::*;
use sailry_client::usage::{Overview, watch_overview};

async fn wait(view: &mut watch::Receiver<Overview>, ready: impl Fn(&Overview) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if ready(&view.borrow_and_update()) {
                return;
            }
            view.changed().await.unwrap();
        }
    })
    .await
    .expect("usage overview deadline");
}

async fn turn(client: &Client, session: SessionId) {
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: "Report shared usage".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    assert_eq!(
        finished(client, session, turn.id).await.runs[0].status,
        Status::Completed
    );
}

#[tokio::test]
async fn preserves_partial_node_reports() {
    let directory = tempfile::tempdir().unwrap();
    let local = Node::start(directory.path().join("local")).await.unwrap();
    let remote = Node::start(directory.path().join("remote")).await.unwrap();
    local
        .link()
        .pair(remote.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let first = Arc::new(Client::new(local.local()));
    let transport = Arc::new(Observed::new(local.link().remote(remote.link().address())));
    let second = Arc::new(Client::new(transport.clone()));
    let server = Server::start(false).await;
    let mut sessions = Vec::new();
    for (index, client) in [&first, &second].into_iter().enumerate() {
        let root = directory.path().join(format!("project-{index}"));
        std::fs::create_dir(&root).unwrap();
        sessions.push(configured(client, &server.endpoint, &root).await.0);
    }
    turn(&first, sessions[0].id).await;
    turn(&second, sessions[1].id).await;
    turn(&second, sessions[1].id).await;
    let now = chrono::Utc::now().timestamp_millis();
    let query = Query {
        start_ms: now - 86_400_000,
        end_ms: now + 86_400_000,
        dimension: Dimension::Model,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    };
    let (sender, mut view) = watch::channel(Overview::default());
    let (refresh, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let task = tokio::spawn(watch_overview(
        vec![first.clone(), first.clone(), second.clone()],
        query.clone(),
        sender,
        stop.clone(),
        requests,
    ));
    wait(&mut view, |view| view.complete).await;
    let complete = view.borrow().clone();
    assert_eq!(complete.sources.len(), 2);
    let summary = complete.summary.as_ref().unwrap();
    assert_eq!(summary.totals.responses, 3);
    assert_eq!(summary.totals.tokens.as_ref().unwrap().input, 36);
    assert_eq!(
        summary
            .days
            .iter()
            .map(|point| point.metrics.responses)
            .sum::<u64>(),
        3
    );
    assert_eq!(summary.groups.len(), 2);
    for point in &summary.days {
        let input: u64 = point.models.values().map(|tokens| tokens.input).sum();
        let output: u64 = point.models.values().map(|tokens| tokens.output).sum();
        assert_eq!(
            input,
            point
                .metrics
                .tokens
                .as_ref()
                .map_or(0, |tokens| tokens.input)
        );
        assert_eq!(
            output,
            point
                .metrics
                .tokens
                .as_ref()
                .map_or(0, |tokens| tokens.output)
        );
        assert!(
            point.models.len() <= 1,
            "matching model IDs share one stack across Nodes"
        );
    }
    for (node, responses) in [(local.id(), 1), (remote.id(), 2)] {
        assert_eq!(
            summary
                .groups
                .iter()
                .find(|group| group.node == node)
                .unwrap()
                .group
                .metrics
                .responses,
            responses
        );
    }
    transport.mode(1);
    refresh.send(()).await.unwrap();
    wait(&mut view, |view| {
        !view.complete
            && view
                .sources
                .iter()
                .any(|source| source.node == remote.id() && source.view.error.is_some())
    })
    .await;
    assert_eq!(view.borrow().summary.as_ref().unwrap().totals.responses, 3);
    turn(&first, sessions[0].id).await;
    wait(&mut view, |view| {
        view.summary
            .as_ref()
            .is_some_and(|summary| summary.totals.responses == 4)
    })
    .await;
    assert!(!view.borrow().complete);
    refresh.send(()).await.unwrap();
    wait(&mut view, |view| view.complete).await;
    let expected = view.borrow().summary.as_ref().unwrap().totals.clone();
    transport.mode(2);
    refresh.send(()).await.unwrap();
    transport.held().await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!view.borrow().complete);
    assert!(
        view.borrow()
            .sources
            .iter()
            .all(|source| !source.view.connected && !source.view.refreshing)
    );
    remote.shutdown().await.unwrap();
    let remote = Node::start(directory.path().join("remote")).await.unwrap();
    let transport = Arc::new(Observed::new(local.link().remote(remote.link().address())));
    transport.mode(2);
    let second = Arc::new(Client::new(transport.clone()));
    let (sender, mut restored) = watch::channel(view.borrow().clone());
    let (_, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let task = tokio::spawn(watch_overview(
        vec![first, second],
        query,
        sender,
        stop.clone(),
        requests,
    ));
    transport.held().await;
    wait(&mut restored, |view| view.refreshing()).await;
    assert!(!restored.borrow().complete);
    assert_eq!(restored.borrow().summary.as_ref().unwrap().totals, expected);
    transport.release();
    wait(&mut restored, |view| view.complete).await;
    assert_eq!(restored.borrow().summary.as_ref().unwrap().totals, expected);
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    stop.cancel();
    task.await.unwrap().unwrap();
    remote.shutdown().await.unwrap();
    local.shutdown().await.unwrap();
}
