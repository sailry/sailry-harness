use super::*;

#[tokio::test]
async fn confirms_removal_on_both_transports() {
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let (session, _) = configured_session(&client, "http://127.0.0.1:1/v1", &root).await;
        for before_observation in [false, true] {
            let selected = if before_observation {
                let Output::Session(created) = client
                    .execute(client.prepare(Command::CreateSession {
                        project: session.project,
                        worktree: Some(session.worktree),
                        config: Some(session.config.clone()),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("session expected")
                };
                client
                    .execute(client.prepare(Command::RemoveSession {
                        session: created.id,
                        expected_revision: created.revision,
                    }))
                    .await
                    .unwrap();
                created
            } else {
                session.clone()
            };
            let (sender, mut receiver) = watch::channel(View::default());
            let (_older, requests) = mpsc::channel(1);
            let stop = CancellationToken::new();
            let observation =
                client.watch_conversation(selected.id, sender, stop.clone(), requests);
            let drive = async {
                if !before_observation {
                    receive(&mut receiver, |view| view.connected).await;
                    assert!(!receiver.borrow().removed);
                    client
                        .execute(client.prepare(Command::RemoveSession {
                            session: selected.id,
                            expected_revision: selected.revision,
                        }))
                        .await
                        .unwrap();
                }
                receive(&mut receiver, |view| view.removed).await;
                assert!(!receiver.borrow().connected);
            };
            let (result, ()) = tokio::join!(observation, drive);
            result.unwrap();
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn receive(receiver: &mut watch::Receiver<View>, predicate: impl Fn(&View) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if predicate(&receiver.borrow_and_update()) {
                return;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .expect("session observation deadline");
}
