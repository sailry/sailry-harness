use super::*;
use sailry_client::{Apply, Projection};
use sailry_protocol::sandbox::Settings;

async fn read(client: &Client) -> Settings {
    let Output::SandboxSettings(settings) = client
        .execute(client.prepare(Command::ReadSandboxSettings))
        .await
        .unwrap()
    else {
        panic!("sandbox settings expected")
    };
    settings
}

async fn toggle(client: &Client, enabled: bool) -> Settings {
    let mut settings = read(client).await;
    settings.enabled = enabled;
    let Output::SandboxSettings(saved) = client
        .execute(client.prepare(Command::SaveSandboxSettings(settings)))
        .await
        .unwrap()
    else {
        panic!("sandbox settings expected")
    };
    saved
}

#[tokio::test]
async fn persists_and_publishes_revision_checked_settings() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transports: [Arc<dyn Transport>; 2] = [node.local(), controller.handle().remote(address)];
    let mut expected = Settings::default();
    for transport in transports {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 0);
        projection.apply(0, updates.next().await.unwrap()).unwrap();
        let mut changed = expected.clone();
        changed.enabled = !changed.enabled;
        let request = client.prepare(Command::SaveSandboxSettings(changed.clone()));
        assert!(request.command.durable());
        let saved = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), saved);
        let Output::SandboxSettings(saved) = saved else {
            panic!("sandbox settings expected")
        };
        assert_eq!(saved.revision, expected.revision + 1);
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveSandboxSettings(changed)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert_ne!(
                    projection.apply(0, updates.next().await.unwrap()).unwrap(),
                    Apply::Recover
                );
                if projection.snapshot().unwrap().sandbox_settings_revision == saved.revision {
                    break;
                }
            }
        })
        .await
        .unwrap();
        expected = saved;
    }
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for transport in [node.local(), controller.handle().remote(address)] {
        assert_eq!(read(&Client::new(transport)).await, expected);
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}

#[tokio::test]
async fn disables_confinement_for_existing_sessions() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for remote in [false, true] {
        for mode in [Permission::Ask, Permission::Project, Permission::Full] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let curl = which::which("curl").expect("sandbox acceptance requires curl");
            let command = format!(
                "{} --silent --max-time 3 http://{address}/ && printf outside > ../allowed.txt && printf '%s' \"$HOME\"",
                quote(&curl)
            );
            let server = Server::scripted_tools(vec![(
                plugin_tool("commands", "run_command"),
                json!({"command":command}),
            )])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            permission(&mut fixture, mode).await;
            assert!(read(&fixture.client).await.enabled);
            toggle(&fixture.client, false).await;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            if mode != Permission::Full {
                let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
                assert_eq!(approval.access, Some(Access::Unrestricted));
                assert_eq!(approval.source, ApprovalSource::User);
                assert!(!approval.can_remember);
                assert!(!fixture.directory.path().join("allowed.txt").exists());
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), listener.accept())
                        .await
                        .is_err()
                );
                toggle(&fixture.client, true).await;
                process::decide(&fixture.client, &approval, Decision::Approve).await;
            }
            let peer = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 2048];
                assert!(stream.read(&mut bytes).await.unwrap() > 0);
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            });
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            tokio::time::timeout(Duration::from_secs(5), peer)
                .await
                .unwrap()
                .unwrap();
            let results = process::results(&page);
            assert_eq!(results.len(), 1, "{page:?}");
            assert_eq!(results[0].outcome, Outcome::Exited(0));
            assert_eq!(results[0].stdout.text, std::env::var("HOME").unwrap());
            assert_eq!(
                std::fs::read_to_string(fixture.directory.path().join("allowed.txt")).unwrap(),
                "outside"
            );
            assert_eq!(page.approvals[0].access, Some(Access::Unrestricted));
            if mode == Permission::Full {
                assert_eq!(page.approvals[0].source, ApprovalSource::Full);
                assert_eq!(page.approvals[0].choice, None);
            }
            // This same session uses the current default on its next command.
            toggle(&fixture.client, true).await;
            std::fs::remove_file(fixture.directory.path().join("allowed.txt")).unwrap();
            server.replace_tools(vec![(plugin_tool("commands", "run_command"), json!({"command":"if printf outside > ../allowed.txt; then exit 91; fi; printf confined"}))]);
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            let mut page = finished(&fixture.client, fixture.session.id, turn.id).await;
            page.approvals.retain(|approval| approval.turn == turn.id);
            page.entries.retain(|entry| entry.turn == turn.id);
            assert_eq!(page.approvals[0].access, Some(Access::Workspace));
            assert_eq!(page.approvals[0].source, ApprovalSource::Sandbox);
            assert_eq!(process::results(&page)[0].outcome, Outcome::Exited(0));
            assert_eq!(process::results(&page)[0].stdout.text, "confined");
            assert!(!fixture.directory.path().join("allowed.txt").exists());
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
