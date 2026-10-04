use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;

#[tokio::test]
async fn recovers_and_clears_partial_output() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let parent = Server::parallel(vec![(
            crate::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({"role": "review", "task": "Stream the delegated answer"}),
        )])
        .await;
        let server = Server::start(true).await;
        let (session, _, _) = setup(&client, &root, &parent, &server).await;
        let mut parent_updates = client.subscribe_conversation(session.id).await.unwrap();
        let mut parent_projection =
            sailry_client::conversation::Projection::new(client.target(), session.id, 0);
        parent_projection
            .apply(0, parent_updates.next().await.unwrap())
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Delegate a streaming task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(1).await;
        let child = children(&client, session.id, 1).await.remove(0);
        let (updates, mut view) = tokio::sync::watch::channel(View::default());
        let stop = CancellationToken::new();
        let (_older, requests) = tokio::sync::mpsc::channel(1);
        let worker = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            async move {
                client
                    .watch_conversation(child.id, updates, stop, requests)
                    .await
            }
        });
        observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.drafts.len() == 1
                    && snapshot.drafts[0].parts == vec![Part::Text("partial-fixture".into())]
            })
        })
        .await;
        let snapshot = view.borrow().snapshot.clone().unwrap();
        let child_turn = snapshot.page.runs[0].turn;
        assert_eq!(snapshot.page.runs[0].status, Status::Running);
        assert_eq!(snapshot.drafts[0].turn, child_turn);
        assert!(!text(&history(&client, child.id).await).contains("partial-fixture"));
        tokio::time::timeout(Duration::from_secs(10), async {
            while parent_projection
                .snapshot()
                .unwrap()
                .page
                .children
                .is_empty()
            {
                parent_projection
                    .apply(0, parent_updates.next().await.unwrap())
                    .unwrap();
            }
        })
        .await
        .unwrap();
        let summary = &parent_projection.snapshot().unwrap().page.children[0];
        assert_eq!(summary.run.turn, child_turn);
        assert_eq!(summary.run.session, child.id);
        assert_eq!(summary.run.status, Status::Running);
        assert_eq!(summary.origin, *child.delegation.as_ref().unwrap().as_ref());
        assert_eq!(summary.name.as_deref(), Some("Review"));

        // Releasing every observer does not end execution or discard its temporary output.
        stop.cancel();
        worker.await.unwrap().unwrap();
        drop(parent_updates);
        controller.handle().disconnect(node.id()).await;
        parent_updates = client.subscribe_conversation(session.id).await.unwrap();
        parent_projection.reconnect(1).unwrap();
        parent_projection
            .apply(1, parent_updates.next().await.unwrap())
            .unwrap();
        assert_eq!(
            parent_projection.snapshot().unwrap().page.children[0]
                .run
                .status,
            Status::Running
        );
        let mut resumed = client.subscribe_conversation(child.id).await.unwrap();
        let initial = resumed.next().await.unwrap();
        let Update::ConversationSnapshot(recovered) = &initial else {
            panic!("snapshot expected")
        };
        assert_eq!(recovered.drafts, snapshot.drafts);
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), child.id, 0);
        projection.apply(0, initial).unwrap();
        client
            .execute(client.prepare(Command::StopTurn { turn: child_turn }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(0, resumed.next().await.unwrap()).unwrap();
                let snapshot = projection.snapshot().unwrap();
                if snapshot.page.runs[0].status == Status::Cancelled {
                    assert!(snapshot.drafts.is_empty());
                    break;
                }
            }
        })
        .await
        .expect("child stop deadline");
        let page = history(&client, child.id).await;
        assert_eq!(projection.snapshot().unwrap().page.as_ref(), &page);
        assert!(!text(&page).contains("partial-fixture"));
        assert_eq!(
            finished(&client, session.id, turn.id).await.runs[0].status,
            Status::Completed
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(parent.requests.lock().unwrap().len(), 2);
        let mut states = vec![];
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let update = parent_updates.next().await.unwrap();
                if let Update::ConversationFrame(Frame {
                    change: Change::Child(child),
                    ..
                }) = &update
                {
                    states.push(child.run.status);
                }
                parent_projection.apply(1, update).unwrap();
                if parent_projection.snapshot().unwrap().page.runs[0].status == Status::Completed {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(states, [Status::Stopping, Status::Cancelled]);
        assert_eq!(
            parent_projection.snapshot().unwrap().page.as_ref(),
            &history(&client, session.id).await
        );
        drop(parent_updates);
        drop(resumed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        assert_eq!(history(&Client::new(node.local()), child.id).await, page);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
    }
}
