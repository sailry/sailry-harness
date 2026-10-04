use super::*;
use sailry_protocol::conversation::search::Query;

fn through(sequence: u64) -> HistoryRequest {
    HistoryRequest::Through(sequence.try_into().unwrap())
}

#[tokio::test]
async fn follows_latest_target() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport.clone());
        let server = Server::start(false).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut turns = Vec::new();
        for index in 0..65 {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 1,
                    message: format!("Search target {index:02} 中文 🙂").into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            super::super::finished(&client, session.id, turn.id).await;
            turns.push(turn.id);
        }
        let matches = client
            .search_conversation(
                session.id,
                Query {
                    text: "Search target 00 中文 🙂".into(),
                    case_sensitive: true,
                    before: None,
                    limit: 1,
                },
            )
            .await
            .unwrap();
        assert_eq!(matches.matches.len(), 1);
        let target = &matches.matches[0];
        assert_eq!(target.turn, turns[0]);
        let canonical = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap();
        let middle = canonical.page.runs[35].sequence;

        // An early request survives initial subscription setup and reports read failure.
        let observed = Arc::new(Observed::new(transport.clone()));
        let watching = Arc::new(Client::new(observed.clone()));
        let (updates, mut view) = watch::channel(View::default());
        let (requests, receiver) = mpsc::channel(8);
        requests.send(through(target.turn_sequence)).await.unwrap();
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let stop = stop.clone();
            async move {
                watching
                    .watch_conversation(session.id, updates, stop, receiver)
                    .await
            }
        });
        super::super::observed(&mut view, |view| view.older_error.is_some()).await;
        assert!(view.borrow().connected);
        assert!(!view.borrow().loading_older);
        assert_eq!(view.borrow().snapshot.as_ref().unwrap().page.runs.len(), 20);
        assert_eq!(observed.reads(), 1);

        // The newer target queued during an in-flight page must not be coalesced away.
        requests.send(through(middle)).await.unwrap();
        observed.wait_held().await;
        requests.send(through(target.turn_sequence)).await.unwrap();
        for _ in 0..4 {
            requests.send(HistoryRequest::Older).await.unwrap();
        }
        let last = submit(&client, session.id).await;
        super::super::observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .last()
                    .is_some_and(|run| run.turn == last && run.status == Status::Completed)
            })
        })
        .await;
        assert!(view.borrow().loading_older);
        assert_eq!(view.borrow().snapshot.as_ref().unwrap().page.runs.len(), 21);
        assert_eq!(observed.cancelled_reads(), 0);
        observed.release();
        super::super::observed(&mut view, |view| {
            !view.loading_older
                && view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.runs.len() == 66 && snapshot.page.next_before.is_none()
                })
        })
        .await;
        assert_eq!(observed.reads(), 4);
        assert!(view.borrow().older_error.is_none());
        let all = view.borrow().snapshot.clone().unwrap();
        assert!(
            all.page
                .entries
                .iter()
                .any(|entry| entry.id == target.entry)
        );
        assert_eq!(observed.cancelled_reads(), 0);
        observed.disconnect();
        super::super::observed(&mut view, |view| !view.connected).await;
        super::super::observed(&mut view, |view| view.connected).await;
        assert_eq!(view.borrow().snapshot.as_ref().unwrap().page, all.page);
        stop.cancel();
        worker.await.unwrap().unwrap();

        // Changing to a nearer result stops at that complete page, not the original target.
        let observed = Arc::new(Observed::new(transport));
        let watching = Arc::new(Client::new(observed.clone()));
        let (updates, mut view) = watch::channel(View::default());
        let (requests, receiver) = mpsc::channel(8);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let stop = stop.clone();
            async move {
                watching
                    .watch_conversation(session.id, updates, stop, receiver)
                    .await
            }
        });
        super::super::observed(&mut view, |view| view.connected).await;
        requests.send(through(target.turn_sequence)).await.unwrap();
        super::super::observed(&mut view, |view| view.older_error.is_some()).await;
        requests.send(through(target.turn_sequence)).await.unwrap();
        observed.wait_held().await;
        requests.send(through(middle)).await.unwrap();
        observed.release();
        super::super::observed(&mut view, |view| {
            !view.loading_older
                && view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.page.runs.len() == 40)
        })
        .await;
        assert_eq!(observed.reads(), 2);
        assert_eq!(
            view.borrow().snapshot.as_ref().unwrap().page.next_before,
            Some(turns[26])
        );
        // An already visible target acknowledges the request without reading any old page.
        requests
            .send(through(canonical.page.runs[64].sequence))
            .await
            .unwrap();
        super::super::observed(&mut view, |view| {
            view.older_error.is_none() && !view.loading_older
        })
        .await;
        assert_eq!(observed.reads(), 2);
        stop.cancel();
        worker.await.unwrap().unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 66);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(
            client
                .read_conversation(session.id, None, 100)
                .await
                .unwrap()
                .page,
            *all.page
        );
        assert_eq!(server.requests.lock().unwrap().len(), 66);
        node.shutdown().await.unwrap();
    }
}
