use super::*;

#[path = "providers/native.rs"]
mod native;
#[path = "providers/switching.rs"]
mod switching;

#[tokio::test]
async fn atomic_save() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let server = Server::start(false).await;
        let (session, mut provider) = configured(&client, &server.endpoint, &root).await;
        provider.models.push(Model {
            id: "fixture-a".into(),
            context: 128,
            output: 32,
            vision: false,
            tools: false,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![sailry_protocol::Effort::High],
            custom_efforts: false,
            default_effort: sailry_protocol::Effort::High,
        });
        provider.default_model = "fixture-a".into();
        let marker = "isolated-agent-credential-fixture";
        let save = client.prepare(Command::SaveProvider {
            provider,
            expected_revision: 1,
            secret: Some(Secret::new(marker.into())),
        });
        assert!(!format!("{save:?}").contains(marker));
        let saved = client.execute(save.clone()).await.unwrap();
        assert_eq!(client.execute(save.clone()).await.unwrap(), saved);
        let Output::Provider(mut provider) = saved else {
            panic!("provider expected")
        };
        let mut config = session.config;
        config.credential = provider.credential.clone();
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "authenticated fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(
            server.authorization.lock().unwrap()[0].as_deref(),
            Some(format!("Bearer {marker}").as_str())
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests[0]["max_completion_tokens"], 32);
        assert_eq!(requests[0]["reasoning_effort"], "high");
        let snapshot = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap();
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(marker));
        assert!(!serde_json::to_string(&page).unwrap().contains(marker));
        let conflict = client.prepare(Command::SaveProvider {
            provider: provider.clone(),
            expected_revision: 1,
            secret: Some(Secret::new("replacement-fixture".into())),
        });
        assert_eq!(
            client.execute(conflict).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let Output::Credentials(credentials) = client
            .execute(client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert_eq!(credentials.len(), 1);
        let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
        let bodies: Vec<Vec<u8>> = db
            .prepare("SELECT body FROM requests UNION ALL SELECT body FROM agent_events")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            bodies
                .iter()
                .all(|body| !String::from_utf8_lossy(body).contains(marker))
        );
        drop(db);
        provider.enabled = false;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "disabled fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
