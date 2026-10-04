use super::*;

async fn wait(client: &Client, session: SessionId, predicate: impl Fn(&Page) -> bool) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = page(client, session).await;
            if predicate(&history) {
                return history;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("attachment lifecycle deadline")
}

async fn start(client: &Client, turn: &QueuedTurn) {
    client
        .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
        .await
        .unwrap();
}

#[tokio::test]
async fn verifies_after_approval() {
    let fixture = Fixture::start().await;
    let bytes = vec![0; adk_core::MAX_INLINE_DATA_SIZE + 1];
    for client in fixture.clients() {
        let file = upload(
            &client,
            fixture.worktree,
            "large.bin",
            "application/octet-stream",
            &bytes,
        )
        .await;
        let server = server::Server::tools(vec![(
            server::plugin_tool("commands", "run_command"),
            json!({"command": "touch unexpected.txt", "attachments": [file.id]}),
        )])
        .await;
        let (session, provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let session = configure(&client, session, provider, Permission::Ask).await;
        let turn = queue(&client, &session, input("Inspect", &[&file])).await;
        start(&client, &turn).await;
        let history = wait(&client, session.id, |page| !page.approvals.is_empty()).await;
        assert!(!fixture.root.join("unexpected.txt").exists());
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .open(
                fixture
                    .node
                    .profile()
                    .join("attachments")
                    .join(file.id.to_string()),
            )
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: history.approvals[0].id,
                decision: sailry_protocol::conversation::Decision::Approve,
            }))
            .await
            .unwrap();
        let history = wait(&client, session.id, |page| {
            page.runs[0].status == Status::Completed
        })
        .await;
        assert!(history.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            matches!(part, Part::ToolResult { result, .. } if result["error"]["code"] == "revision_conflict")
        }));
        assert!(!fixture.root.join("unexpected.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }
    fixture.close().await;
}

#[tokio::test]
async fn releases_copies() {
    for remote in [false, true] {
        for shutdown in [false, true] {
            let mut fixture = Fixture::start().await;
            let client = fixture
                .clients()
                .into_iter()
                .nth(usize::from(remote))
                .unwrap();
            let file = upload(
                &client,
                fixture.worktree,
                "input.bin",
                "application/octet-stream",
                b"\0original",
            )
            .await;
            let server = server::Server::tools(vec![(server::plugin_tool("commands", "run_command"), json!({"command": "printf '%s' \"$SAILRY_ATTACHMENTS\" > staged.txt && touch ready.txt; sleep 60; touch unexpected.txt", "attachments": [file.id]}))]).await;
            let (session, provider) = session(
                &client,
                fixture.worktree,
                &server.endpoint,
                ModelApi::ChatCompletions,
            )
            .await;
            let session = configure(&client, session, provider, Permission::Full).await;
            let turn = queue(&client, &session, input("Inspect", &[&file])).await;
            start(&client, &turn).await;
            wait(&client, session.id, |_| {
                fixture.root.join("ready.txt").exists()
            })
            .await;
            let staged = std::fs::read_to_string(fixture.root.join("staged.txt")).unwrap();
            assert_eq!(
                std::path::Path::new(&staged).parent(),
                Some(fixture.node.profile())
            );
            assert!(
                std::path::Path::new(&staged)
                    .join(file.id.to_string())
                    .exists()
            );
            if shutdown {
                let profile = fixture.node.profile().to_owned();
                fixture.node.shutdown().await.unwrap();
                fixture.node = Node::start(profile).await.unwrap();
                let client = fixture
                    .clients()
                    .into_iter()
                    .nth(usize::from(remote))
                    .unwrap();
                assert_eq!(
                    page(&client, session.id).await.runs[0].status,
                    Status::Interrupted
                );
            } else {
                client
                    .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                    .await
                    .unwrap();
                wait(&client, session.id, |page| {
                    page.runs[0].status == Status::Cancelled
                })
                .await;
            }
            assert!(!std::path::Path::new(&staged).exists());
            assert!(!fixture.root.join("unexpected.txt").exists());
            assert_eq!(download(&fixture.clients()[0], &file).await, b"\0original");
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.close().await;
        }
    }
}
