use super::*;
use sailry_protocol::conversation::Change;
use std::collections::BTreeSet;

#[tokio::test]
async fn publishes_latest_usage_after_committed_entries() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = Server::start(false).await;
        let (session, _) = configured_session(&client, &server.endpoint, &root).await;
        let mut stream = client.subscribe_conversation(session.id).await.unwrap();
        assert!(matches!(
            stream.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let (entered, release) = node.hold_statistics_fixture(session.id).unwrap();
        let mut completed = BTreeSet::new();
        let mut entries = BTreeSet::new();
        for index in 0..2 {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: format!("Statistics batch {index}").into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            if index == 0 {
                tokio::time::timeout(Duration::from_secs(10), entered.notified())
                    .await
                    .unwrap();
            }
            // Writer admission, canonical appends and terminal publication all progress
            // while the first derived snapshot remains suspended.
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            completed.insert(turn.id);
            entries.extend(page.entries.iter().map(|entry| entry.id.clone()));
        }
        let expected = statistics(&client, session.id).await;
        assert_eq!(expected.responses, 2);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !entries.is_empty() || !completed.is_empty() {
                let Update::ConversationFrame(frame) = stream.next().await.unwrap() else {
                    panic!("ordered frame expected")
                };
                match frame.change {
                    Change::Entry(entry) => {
                        entries.remove(&entry.id);
                    }
                    Change::Run(run) if run.status == Status::Completed => {
                        completed.remove(&run.turn);
                    }
                    // A configuration snapshot already queued before the gate may
                    // contain zero responses, but no partial batch can escape it.
                    Change::Statistics(statistics) => assert_eq!(statistics.responses, 0),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        release.send(()).unwrap();
        let latest = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Update::ConversationFrame(frame) = stream.next().await.unwrap()
                    && let Change::Statistics(statistics) = frame.change
                {
                    break statistics;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(*latest, expected);
        drop(stream);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
