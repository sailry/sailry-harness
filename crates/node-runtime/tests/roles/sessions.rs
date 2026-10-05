use super::*;

const MESSAGE: &str = "Remain queued 中文 🙂";

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn snapshot(client: &Client) -> sailry_protocol::Snapshot {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot
}

async fn provider(client: &Client, secret: Option<&str>) -> Provider {
    let Output::Provider(provider) = execute(
        client,
        Command::SaveProvider {
            expected_revision: 0,
            secret: secret.map(|secret| Secret::new(secret.into())),
            provider: Provider {
                oauth: None,
                options: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Frozen role fixture".into(),
                api: ModelApi::ChatCompletions,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: "http://127.0.0.1:9/v1".into(),
                enabled: true,
                default_model: "fixture".into(),
                credential: None,
                models: vec![Model {
                    id: "fixture".into(),
                    context: 128,
                    output: 32,
                    vision: false,
                    tools: true,
                    reasoning: true,
                    web_search: false,
                    generates: vec![],
                    efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
                    custom_efforts: false,
                    default_effort: sailry_protocol::Effort::High,
                }],
            },
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    provider
}

async fn create(client: &Client, root: &std::path::Path, provider: &Provider) -> Session {
    let Output::Project(project) = execute(
        client,
        Command::RegisterProject {
            name: "Session role fixture".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    execute(
        client,
        Command::SetDefaults {
            expected_revision: 0,
            config: SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture".into(),
                effort: Effort::Low,
                mode: sailry_protocol::WorkMode::Code,
                permission: Permission::Ask,
                credential: provider.credential.clone(),
            },
        },
    )
    .await;
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: None,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

async fn fixed(client: &Client, provider: &Provider, key: &str) -> role::Profile {
    let mut role = profile(key);
    role.model = Some(role::Model {
        provider: provider.id,
        model: "fixture".into(),
        effort: None,
    });
    role.skills = vec!["fixture/review".into()];
    put(client, role, 0).await.unwrap()
}

async fn queue(client: &Client, session: &Session) -> QueuedTurn {
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: MESSAGE.into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    turn
}

fn queued_activity(session: &Session, turn: &QueuedTurn) -> sailry_protocol::activity::Summary {
    sailry_protocol::activity::Summary {
        title: MESSAGE.into(),
        attention: Default::default(),
        run: Some(Run {
            worktree: session.worktree,
            turn: turn.id,
            kind: RunKind::Task,
            session: session.id,
            sequence: 1,
            revision: turn.revision,
            status: Status::Queued,
            error: None,
            started_ms: None,
            finished_ms: None,
            origin: None,
        }),
        waiting: None,
        queued: 1,
    }
}

#[tokio::test]
async fn preserves_frozen_revisions() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let provider = provider(&client, None).await;
        let first = fixed(&client, &provider, "review").await;
        let inherited = put(&client, profile("analysis"), 0).await.unwrap();
        let mut session = create(&client, directory.path(), &provider).await;
        assert_eq!(session.roles.profiles, [inherited.clone(), first.clone()]);
        assert_eq!(session.roles.providers, std::slice::from_ref(&provider));
        let turn = queue(&client, &session).await;
        session.activity = queued_activity(&session, &turn);
        assert_eq!(turn.roles, session.roles);
        let mut changed = first.clone();
        changed.key = "renamed".into();
        changed.instructions = "Updated instructions 中文 🙂".into();
        changed.max_turns = Some(7);
        changed.model.as_mut().unwrap().effort = Some(Effort::Low);
        let changed = put(&client, changed, 1).await.unwrap();
        execute(
            &client,
            Command::RemoveRole {
                role: inherited.id,
                expected_revision: 1,
            },
        )
        .await;
        let mut updated_provider = provider.clone();
        updated_provider.endpoint = "http://127.0.0.1:9/changed".into();
        execute(
            &client,
            Command::PutProvider {
                provider: updated_provider,
                expected_revision: 1,
            },
        )
        .await;
        assert_eq!(
            snapshot(&client).await.sessions,
            std::slice::from_ref(&session)
        );
        for (roles, expected) in [
            (vec![first.reference()], ErrorCode::RevisionConflict),
            (vec![inherited.reference()], ErrorCode::NotFound),
            (
                vec![first.reference(), first.reference()],
                ErrorCode::InvalidRequest,
            ),
            (
                (0..=role::MAX_PROFILES)
                    .map(|_| role::Reference {
                        id: RoleId::new(),
                        revision: 1,
                    })
                    .collect(),
                ErrorCode::InvalidRequest,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::SetSessionRoles {
                        session: session.id,
                        expected_revision: session.revision,
                        roles,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                expected,
            );
            assert_eq!(
                snapshot(&client).await.sessions,
                std::slice::from_ref(&session),
            );
        }
        let request = client.prepare(Command::SetSessionRoles {
            session: session.id,
            expected_revision: 1,
            roles: vec![changed.reference()],
        });
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        let Output::Session(revised) = client.execute(request.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(revised.revision, 2);
        assert_eq!(revised.roles.profiles, [changed]);
        assert_eq!(
            revised.roles.providers[0].endpoint,
            "http://127.0.0.1:9/changed"
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::Session(revised.clone())
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionRoles {
                    session: session.id,
                    expected_revision: 1,
                    roles: vec![],
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionRoles {
                    session: session.id,
                    expected_revision: 2,
                    roles: vec![first.reference(), first.reference()],
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let second = queue(&client, &revised).await;
        assert_eq!(second.roles, revised.roles);
        let Output::QueuedMessage(queued) =
            execute(&client, Command::ReadQueuedTurn { turn: turn.id }).await
        else {
            panic!("queued message expected")
        };
        assert_eq!(queued.turn.roles, session.roles);
        let Output::Session(configured) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: 2,
                config: SessionConfig {
                    assistant: None,
                    resource: None,
                    effort: Effort::High,
                    ..revised.config.clone()
                },
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(configured.roles, revised.roles);
        let Output::Session(cleared) = execute(
            &client,
            Command::SetSessionRoles {
                session: session.id,
                expected_revision: 3,
                roles: vec![],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(cleared.roles, role::Snapshot::default());
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let restored = snapshot(&client).await;
        assert_eq!(restored.sessions, [cleared]);
        assert_eq!(restored.turns, [turn, second]);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Session(revised)
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn imports_private_providers() {
    for remote in [false, true] {
        let (directory, target, controller, destination) = fixture(remote).await;
        let source = Node::start(directory.path().join("source")).await.unwrap();
        source
            .link()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let origin = Client::new(source.local());
        let parent = provider(&origin, None).await;
        let fixed_provider = provider(&origin, Some("isolated-role-secret")).await;
        let first = fixed(&origin, &fixed_provider, "review").await;
        let second = fixed(&origin, &fixed_provider, "analysis").await;
        let inherited = put(&origin, profile("inherit"), 0).await.unwrap();
        let local = create(&origin, directory.path(), &parent).await;
        let target_provider = provider(&destination, None).await;
        let own = put(&destination, profile("target"), 0).await.unwrap();
        let default = create(&destination, directory.path(), &target_provider).await;
        let before = snapshot(&destination).await;
        let request = origin.prepare(Command::CreateSessionAt {
            target: target.id(),
            project: default.project,
            worktree: Some(default.worktree),
            config: Box::new(local.config.clone()),
            provider_revision: parent.revision,
        });
        let Output::Session(mut imported) = origin.execute(request.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(
            origin.execute(request).await.unwrap(),
            Output::Session(imported.clone())
        );
        assert_eq!(imported.roles.providers.len(), 1);
        let provider = &imported.roles.providers[0];
        assert_ne!(provider.id, fixed_provider.id);
        assert_eq!(provider.endpoint, fixed_provider.endpoint);
        assert_eq!(provider.models, fixed_provider.models);
        let reference = provider.credential.as_ref().unwrap();
        assert_eq!(reference.node, target.id());
        assert_ne!(Some(reference), fixed_provider.credential.as_ref());
        let mut expected = vec![second, inherited, first];
        for role in &mut expected {
            if let Some(model) = &mut role.model {
                model.provider = provider.id;
            }
        }
        assert_eq!(imported.roles.profiles, expected);
        let after = snapshot(&destination).await;
        assert_eq!(after.providers, before.providers);
        assert_eq!(after.defaults, before.defaults);
        assert_eq!(after.roles, [own]);
        assert!(
            !serde_json::to_string(&after)
                .unwrap()
                .contains("isolated-role-secret")
        );
        let Output::Credentials(credentials) =
            execute(&destination, Command::ListCredentials).await
        else {
            panic!("credentials expected")
        };
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].id, reference.id);
        assert_eq!(credentials[0].provider, provider.id);
        source.shutdown().await.unwrap();
        let queued = queue(&destination, &imported).await;
        imported.activity = queued_activity(&imported, &queued);
        assert_eq!(queued.roles, imported.roles);
        target.shutdown().await.unwrap();
        let target = Node::start(directory.path().join("node")).await.unwrap();
        let destination = Client::new(if remote {
            controller.handle().remote(target.link().address())
        } else {
            target.local()
        });
        let restored = snapshot(&destination).await;
        assert_eq!(restored.sessions, [imported, default]);
        assert_eq!(restored.turns, [queued]);
        controller.close().await.unwrap();
        target.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_inconsistent_imports() {
    let (directory, target, controller, destination) = fixture(true).await;
    let source = Node::start(directory.path().join("source")).await.unwrap();
    source
        .link()
        .pair(target.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let origin = Client::new(source.local());
    let peer = Client::new(source.link().remote(target.link().address()));
    let parent = provider(&origin, None).await;
    let selected = provider(&origin, Some("isolated-import-role-secret")).await;
    fixed(&origin, &selected, "review").await;
    let local = create(&origin, directory.path(), &parent).await;
    let target_provider = provider(&destination, None).await;
    let default = create(&destination, directory.path(), &target_provider).await;
    let bundle = SessionImport {
        source: source.id(),
        project: default.project,
        worktree: Some(default.worktree),
        config: local.config,
        provider: parent,
        secret: None,
        expires_at_ms: None,
        roles: local.roles,
        role_credentials: vec![role::ProviderSecret {
            provider: selected.id,
            secret: Secret::new("isolated-import-role-secret".into()),
            expires_at_ms: None,
        }],
    };
    let mut invalid = Vec::new();
    let mut missing = bundle.clone();
    missing.role_credentials.clear();
    invalid.push(missing);
    let mut duplicate = bundle.clone();
    duplicate
        .role_credentials
        .push(duplicate.role_credentials[0].clone());
    invalid.push(duplicate);
    let mut unexpected = bundle.clone();
    unexpected.role_credentials[0].provider = ProviderId::new();
    invalid.push(unexpected);
    let mut foreign = bundle.clone();
    foreign.roles.providers[0].credential.as_mut().unwrap().node = target.id();
    invalid.push(foreign);
    let mut expired = bundle.clone();
    expired.role_credentials[0].expires_at_ms = Some(1);
    invalid.push(expired);
    let mut endpoint = bundle.clone();
    endpoint.roles.providers[0].endpoint = "file:///invalid".into();
    invalid.push(endpoint);
    let mut roles = bundle.clone();
    roles.roles.profiles.push(roles.roles.profiles[0].clone());
    invalid.push(roles);
    for bundle in invalid {
        assert_eq!(
            peer.execute(peer.prepare(Command::ImportSession(Box::new(bundle))))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            execute(&destination, Command::ListCredentials).await,
            Output::Credentials(vec![])
        );
        assert_eq!(
            snapshot(&destination).await.sessions,
            std::slice::from_ref(&default)
        );
    }
    let Output::Session(imported) = execute(&peer, Command::ImportSession(Box::new(bundle))).await
    else {
        panic!("session expected")
    };
    let reference = imported.roles.providers[0].credential.as_ref().unwrap();
    execute(
        &destination,
        Command::RevokeCredential {
            id: reference.id,
            expected_revision: 1,
        },
    )
    .await;
    assert_eq!(snapshot(&destination).await.sessions, [imported, default]);
    source.shutdown().await.unwrap();
    controller.close().await.unwrap();
    target.shutdown().await.unwrap();
    let db =
        rusqlite::Connection::open(directory.path().join("node/storage/node.sqlite3")).unwrap();
    for table in ["requests", "events"] {
        let body = format!(
            "SELECT count(*) FROM {table} WHERE CAST(body AS TEXT) LIKE '%isolated-import-role-secret%'"
        );
        assert_eq!(
            db.query_row(&body, [], |row| row.get::<_, i64>(0)).unwrap(),
            0
        );
    }
}
