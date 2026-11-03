use super::*;
use sailry_client::conversation::{HistoryRequest, View};
use sailry_link::CancellationToken;
use tokio::sync::{mpsc, watch};

async fn observe(updates: &mut watch::Receiver<View>, ready: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = updates.borrow_and_update().clone();
            assert!(view.error.is_none(), "{:#?}", view.error);
            assert!(
                view.statistics_error.is_none(),
                "{:#?}",
                view.statistics_error
            );
            if ready(&view) {
                return view;
            }
            updates
                .changed()
                .await
                .expect("conversation observer stopped");
        }
    })
    .await
    .expect("conversation observation deadline")
}

#[tokio::test]
async fn publishes_history_before_cold_statistics() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(node.local());
        let server = Server::start(false).await;
        let (session, _) = configured_session(&client, &server.endpoint, &root).await;
        for index in 0..24 {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: format!("Retained history {index}").into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        }
        let expected = statistics(&client, session.id).await;
        let recent = client
            .read_conversation(session.id, None, 20)
            .await
            .unwrap()
            .page;
        assert_eq!(recent.runs.len(), 20);
        assert!(recent.next_before.is_some());
        assert!(!recent.entries.is_empty());
        let requests = server.requests.lock().unwrap().len();
        drop(client);
        node.shutdown().await.unwrap();

        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let (entered, release) = node.hold_statistics_read_fixture(session.id).unwrap();
        let (sender, mut updates) = watch::channel(View::default());
        let (_older, history) = mpsc::channel::<HistoryRequest>(1);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            async move {
                client
                    .watch_conversation(session.id, sender, stop, history)
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(10), entered.notified())
            .await
            .expect("cold statistics read was not scheduled");
        let pending = observe(&mut updates, |view| {
            view.connected && view.snapshot.is_some()
        })
        .await;
        assert!(pending.statistics.is_none());
        let initial = pending.snapshot.as_ref().unwrap();
        assert!(initial.statistics.is_none());
        assert_eq!(initial.page.as_ref(), &recent);

        release.send(()).unwrap();
        let ready = observe(&mut updates, |view| view.statistics.is_some()).await;
        let published = ready.snapshot.as_ref().unwrap();
        assert_eq!(published.statistics.as_ref(), Some(&expected));
        assert_eq!(ready.statistics.as_deref(), Some(&expected));
        assert_eq!(ready.generation, pending.generation);
        assert!(Arc::ptr_eq(&published.page, &initial.page));
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        stop.cancel();
        worker.await.unwrap().unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
