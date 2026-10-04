use super::*;
use sailry_protocol::conversation::search::Query;

fn query(text: &str) -> Query {
    Query {
        text: text.into(),
        case_sensitive: false,
        before: None,
        limit: 7,
    }
}

async fn submit(client: &Client, session: SessionId, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let page = finished(client, session, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    turn.id
}

#[tokio::test]
async fn searches_unloaded_history() {
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
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = Server::start(false).await;
        let (session, _) = configured(&client, &server.endpoint, &root).await;
        let first = submit(
            &client,
            session.id,
            "Search ÄBC 中文 🙂 and literal .* [text]",
        )
        .await;
        let mut original = Vec::new();
        for index in 0..24 {
            original.push(submit(&client, session.id, &format!("History fixture {index}")).await);
        }
        let recent = client
            .read_conversation(session.id, None, 20)
            .await
            .unwrap();
        assert!(!recent.page.runs.iter().any(|run| run.turn == first));
        let found = client
            .search_conversation(session.id, query("äbc 中文 🙂"))
            .await
            .unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].turn, first);
        assert_eq!(
            &found.matches[0].snippet[found.matches[0].highlight.clone()],
            "ÄBC 中文 🙂"
        );
        assert!(
            client
                .search_conversation(
                    session.id,
                    Query {
                        case_sensitive: true,
                        ..query("äbc")
                    }
                )
                .await
                .unwrap()
                .matches
                .is_empty()
        );
        assert_eq!(
            client
                .search_conversation(session.id, query(".*"))
                .await
                .unwrap()
                .matches
                .len(),
            1
        );
        let mut page = client
            .search_conversation(session.id, query("History"))
            .await
            .unwrap();
        assert_eq!(page.matches.len(), 7);
        let inserted = submit(&client, session.id, "History inserted during search").await;
        let mut matches = page.matches;
        while let Some(before) = page.next_before {
            page = client
                .search_conversation(
                    session.id,
                    Query {
                        before: Some(before),
                        ..query("History")
                    },
                )
                .await
                .unwrap();
            matches.extend(page.matches);
        }
        original.reverse();
        assert_eq!(
            matches.iter().map(|found| found.turn).collect::<Vec<_>>(),
            original
        );
        assert!(!matches.iter().any(|found| found.turn == inserted));
        let Output::Snapshot(before) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(
            client
                .search_conversation(session.id, query("absent"))
                .await
                .unwrap()
                .matches
                .is_empty()
        );
        let Output::Snapshot(after) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(before, after);
        assert_eq!(server.requests.lock().unwrap().len(), 26);

        let slow = Server::start(true).await;
        let other = fixture.path().join("other-project");
        std::fs::create_dir(&other).unwrap();
        let (pending, _) = configured(&client, &slow.endpoint, &other).await;
        let mut stream = client.subscribe_conversation(pending.id).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), pending.id, 0);
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: pending.id,
                expected_revision: 1,
                message: "Transient search fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(0, stream.next().await.unwrap()).unwrap();
                if projection
                    .snapshot()
                    .is_some_and(|snapshot| !snapshot.drafts.is_empty())
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            client
                .search_conversation(pending.id, query("partial-fixture"))
                .await
                .unwrap()
                .matches
                .is_empty()
        );
        assert!(
            client
                .search_conversation(pending.id, query("äbc"))
                .await
                .unwrap()
                .matches
                .is_empty()
        );
        assert_eq!(
            client
                .search_conversation(
                    pending.id,
                    Query {
                        before: Some(found.matches[0].sequence),
                        ..query("search")
                    }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        finished(&client, pending.id, turn.id).await;
        drop(stream);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client
                .search_conversation(session.id, query("äbc 中文 🙂"))
                .await
                .unwrap(),
            found
        );
        assert!(
            client
                .search_conversation(pending.id, query("partial-fixture"))
                .await
                .unwrap()
                .matches
                .is_empty()
        );
        assert_eq!(server.requests.lock().unwrap().len(), 26);
        assert_eq!(slow.requests.lock().unwrap().len(), 1);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
