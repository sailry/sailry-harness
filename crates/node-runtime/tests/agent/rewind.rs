use super::*;
use sailry_client::conversation::{HistoryRequest, View};
use sailry_link::CancellationToken;
use serde_json::json;
use tokio::sync::{mpsc, watch};

async fn submit(client: &Client, session: &Session, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let page = finished(client, session.id, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    turn.id
}

fn query(text: &str) -> conversation::search::Query {
    conversation::search::Query {
        text: text.into(),
        case_sensitive: true,
        before: None,
        limit: 100,
    }
}

#[tokio::test]
async fn recovers_subscribers_after_rewind() {
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
        let server = Server::tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"rewind.txt","text":"Written once 中文 🙂","expected_revision":null}),
        )])
        .await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let first = submit(&client, &session, "Retained first 中文 🙂").await;
        let mut head = first;
        for index in 1..24 {
            head = submit(&client, &session, &format!("Removed message {index}")).await;
        }
        let before = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page;
        assert_eq!(before.runs.len(), 24);
        assert_eq!(before.entries.len(), 51);
        assert_eq!(before.approvals.len(), 1);
        let delayed = Arc::new(super::paging::transport::Observed::new(transport.clone()));
        let watching = Arc::new(Client::new(delayed.clone()));
        let (updates, mut view) = watch::channel(View::default());
        let (older, requests) = mpsc::channel(8);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let watching = watching.clone();
            let stop = stop.clone();
            async move {
                watching
                    .watch_conversation(session.id, updates, stop, requests)
                    .await
            }
        });
        observed(&mut view, |view| view.connected).await;
        assert_eq!(view.borrow().snapshot.as_ref().unwrap().page.runs.len(), 20);
        older.send(HistoryRequest::Older).await.unwrap();
        observed(&mut view, |view| view.older_error.is_some()).await;
        older.send(HistoryRequest::Older).await.unwrap();
        delayed.wait_held().await;
        assert!(view.borrow().loading_older);
        let mut direct = client.subscribe_conversation(session.id).await.unwrap();
        assert!(matches!(
            direct.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let request = client.prepare(Command::RewindConversation {
            session: session.id,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        });
        let admission = transport.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission);
        let output = client.execute(request.clone()).await.unwrap();
        let Output::Rewound(result) = &output else {
            panic!("rewind expected")
        };
        assert_eq!(result.revision, 2);
        assert_eq!(result.backup.config, session.config);
        assert_eq!(
            result.backup.fork,
            Some(Fork {
                session: session.id,
                through: head
            })
        );
        assert_eq!(direct.next().await.unwrap(), Update::ResetRequired);
        observed(&mut view, |view| {
            view.connected
                && view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.page.revision == 2)
        })
        .await;
        let restored = view.borrow().snapshot.clone().unwrap();
        assert_eq!(restored.page.runs.len(), 1);
        assert_eq!(restored.page.entries, before.entries[..5]);
        assert_eq!(restored.page.approvals, before.approvals);
        assert!(restored.missing.is_empty());
        assert_eq!(view.borrow().calls.len(), 1);
        delayed.release();
        let retained = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page;
        assert_eq!(*restored.page, retained);
        let backup = client
            .read_conversation(result.backup.id, None, 100)
            .await
            .unwrap()
            .page;
        assert_eq!(backup.entries, before.entries);
        assert_eq!(backup.runs.len(), 24);
        assert!(backup.runs.iter().all(|run| run.origin == Some(session.id)));
        let found = client
            .search_conversation(session.id, query("Removed message"))
            .await
            .unwrap();
        assert_eq!(found.revision, 2);
        assert!(found.matches.is_empty());
        assert_eq!(
            client
                .search_conversation(result.backup.id, query("Removed message"))
                .await
                .unwrap()
                .matches
                .len(),
            23
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadTurn {
                    session: session.id,
                    turn: head,
                    expected_revision: 1,
                    before: None,
                    limit: 100
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadTurn {
                    session: session.id,
                    turn: head,
                    expected_revision: 2,
                    before: None,
                    limit: 100
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            client
                .read_conversation(session.id, Some(head), 20)
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        let mut altered = request.clone();
        if let Command::RewindConversation { through, .. } = &mut altered.command {
            *through = None;
        }
        assert_eq!(
            client.execute(altered).await.unwrap_err().code,
            ErrorCode::Conflict
        );

        // A retained request result must not issue a second reset or create another backup.
        let mut stable = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(initial) = stable.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(client.execute(request.clone()).await.unwrap(), output);
        let mut stable_projection =
            sailry_client::conversation::Projection::new(client.target(), session.id, 0);
        stable_projection
            .apply(0, Update::ConversationSnapshot(initial))
            .unwrap();
        let other = Client::new(node.local());
        let next = submit(&other, &session, "Independent continuation").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let update = stable.next().await.unwrap();
                assert_ne!(update, Update::ResetRequired);
                stable_projection.apply(0, update).unwrap();
                if stable_projection
                    .snapshot()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == next && run.status == Status::Completed)
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == next && run.status == Status::Completed)
            })
        })
        .await;
        let resumed = history(&other, session.id).await;
        assert_eq!(resumed.runs.len(), 2);
        assert_eq!(resumed.revision, 2);
        assert_eq!(resumed.entries.len(), 7);
        assert_eq!(*view.borrow().snapshot.as_ref().unwrap().page, resumed);
        let body = server.requests.lock().unwrap().last().unwrap().clone();
        let contents: Vec<_> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect();
        assert!(contents.contains(&"Retained first 中文 🙂"));
        assert!(contents.contains(&"Independent continuation"));
        assert!(
            !contents
                .iter()
                .any(|message| message.starts_with("Removed message"))
        );
        assert_eq!(server.requests.lock().unwrap().len(), 26);
        assert_eq!(
            std::fs::read_to_string(root.join("rewind.txt")).unwrap(),
            "Written once 中文 🙂"
        );
        assert_eq!(
            client
                .read_conversation(result.backup.id, None, 100)
                .await
                .unwrap()
                .page,
            backup
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::RewindConversation {
                    session: session.id,
                    through: None,
                    expected_head: first,
                    expected_revision: 2
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Snapshot(before_restart) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let persisted = before_restart
            .sessions
            .iter()
            .find(|current| current.id == session.id)
            .unwrap()
            .clone();
        stop.cancel();
        worker.await.unwrap().unwrap();
        drop(stable);
        drop(direct);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let reopened = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(reopened.execute(request).await.unwrap(), output);
        assert_eq!(history(&reopened, session.id).await, resumed);
        assert_eq!(
            reopened
                .read_conversation(result.backup.id, None, 100)
                .await
                .unwrap()
                .page,
            backup
        );
        let Output::Snapshot(snapshot) = reopened
            .execute(reopened.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|current| current.id == session.id)
                .unwrap(),
            &persisted
        );
        assert_eq!(server.requests.lock().unwrap().len(), 26);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
