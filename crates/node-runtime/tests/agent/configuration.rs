use super::*;
#[path = "configuration/validation.rs"]
mod validation;

pub(super) async fn snapshot(client: &Client) -> sailry_protocol::Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

async fn credentials(client: &Client) -> Vec<Credential> {
    let Output::Credentials(credentials) = client
        .execute(client.prepare(Command::ListCredentials))
        .await
        .unwrap()
    else {
        panic!("credentials expected")
    };
    credentials
}

#[tokio::test]
async fn independent_resume() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let source = Node::start(fixture.path().join("source")).await.unwrap();
        let target_path = fixture.path().join("target");
        let target = Node::start(&target_path).await.unwrap();
        let source_id = source.id();
        source
            .link()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let destination = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            target.local()
        });
        let origin = Client::new(source.local());
        let selected = Server::start(false).await;
        let defaults = Server::start(false).await;
        let (_, mut provider) = configured(&origin, &selected.endpoint, &root).await;
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
        let marker = "isolated-forwarded-credential";
        let Output::Provider(provider) = origin
            .execute(origin.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new(marker.into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: "fixture-a".into(),
            effort: Effort::High,
            mode: sailry_protocol::WorkMode::Code,
            permission: sailry_protocol::Permission::Ask,
            credential: provider.credential.clone(),
        };
        let (default_session, default_provider) =
            configured(&destination, &defaults.endpoint, &root).await;
        destination
            .execute(destination.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: default_session.config.clone(),
            }))
            .await
            .unwrap();
        let baseline = snapshot(&destination).await;
        let create = origin.prepare(Command::CreateSessionAt {
            target: target.id(),
            project: default_session.project,
            worktree: Some(default_session.worktree),
            config: Box::new(config.clone()),
            provider_revision: provider.revision,
        });
        let Output::Session(session) = origin.execute(create.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(
            origin.execute(create.clone()).await.unwrap(),
            Output::Session(session.clone())
        );
        let imported = session.profile.as_ref().unwrap();
        assert_eq!(imported.source, source_id);
        assert_eq!(imported.provider.models, provider.models);
        assert_eq!(imported.provider.endpoint, provider.endpoint);
        assert_ne!(session.config.provider, provider.id);
        let reference = session.config.credential.as_ref().unwrap();
        assert_eq!(reference.node, target.id());
        assert_ne!(Some(reference), provider.credential.as_ref());
        let current = snapshot(&destination).await;
        assert_eq!(current.defaults, baseline.defaults);
        assert_eq!(current.providers, vec![default_provider]);
        assert_eq!(current.sessions.len(), baseline.sessions.len() + 1);
        assert!(current.sessions.contains(&session));
        assert!(
            !snapshot(&origin)
                .await
                .sessions
                .iter()
                .any(|value| value.id == session.id)
        );
        assert_eq!(credentials(&destination).await.len(), 1);
        assert!(!serde_json::to_string(&current).unwrap().contains(marker));
        for node in [&source, &target] {
            let db =
                rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
            let mut query = db
                .prepare("SELECT body FROM requests UNION ALL SELECT body FROM events")
                .unwrap();
            for body in query.query_map([], |row| row.get::<_, Vec<u8>>(0)).unwrap() {
                assert!(!String::from_utf8_lossy(&body.unwrap()).contains(marker));
            }
        }
        let credential = credentials(&origin).await.pop().unwrap();
        origin
            .execute(origin.prepare(Command::RevokeCredential {
                id: credential.id,
                expected_revision: credential.revision,
            }))
            .await
            .unwrap();
        let retry = origin.prepare(Command::CreateSessionAt {
            target: target.id(),
            project: session.project,
            worktree: Some(session.worktree),
            config: Box::new(config),
            provider_revision: provider.revision,
        });
        assert_eq!(
            origin.execute(retry).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(credentials(&destination).await.len(), 1);
        source.shutdown().await.unwrap();
        target.shutdown().await.unwrap();
        // A different controller resumes after both execution and source shutdown.
        let target = Node::start(&target_path).await.unwrap();
        let destination = Client::new(if remote {
            controller.handle().remote(target.link().address())
        } else {
            target.local()
        });
        assert!(snapshot(&destination).await.sessions.contains(&session));
        let Output::QueuedTurn(turn) = destination
            .execute(destination.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "resume independently".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&destination, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(defaults.requests.lock().unwrap().is_empty());
        assert_eq!(
            selected.requests.lock().unwrap()[0]["max_completion_tokens"],
            32
        );
        assert_eq!(
            selected.authorization.lock().unwrap()[0].as_deref(),
            Some(format!("Bearer {marker}").as_str())
        );
        assert!(!serde_json::to_string(&page).unwrap().contains(marker));
        let queued = destination.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "frozen revision".into(),
        });
        let Output::QueuedTurn(queued) = destination.execute(queued).await.unwrap() else {
            panic!("turn expected")
        };
        destination
            .execute(destination.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: 1,
                config: default_session.config.clone(),
            }))
            .await
            .unwrap();
        assert_eq!(
            destination
                .execute(destination.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: 1,
                    config: session.config.clone(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        destination
            .execute(destination.prepare(Command::StartQueuedTurn { turn: queued.id }))
            .await
            .unwrap();
        assert_eq!(
            finished(&destination, session.id, queued.id)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(selected.requests.lock().unwrap().len(), 2);
        let Output::QueuedTurn(next) = destination
            .execute(destination.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "explicit target configuration".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert_eq!(
            finished(&destination, session.id, next.id)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(defaults.requests.lock().unwrap().len(), 1);
        assert!(
            snapshot(&destination)
                .await
                .sessions
                .iter()
                .find(|value| value.id == session.id)
                .unwrap()
                .profile
                .is_none()
        );
        target.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
