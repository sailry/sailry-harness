use super::*;

#[tokio::test]
async fn requires_destination_identity() {
    for api in [ModelApi::Bedrock, ModelApi::Vertex] {
        let fixture = Fixture::new(false, api, "http://127.0.0.1:9/must-not-call").await;
        let mut provider = fixture.provider.clone();
        provider.authentication = Authentication::Host;
        provider.credential = None;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = fixture.session.config.clone();
        config.credential = None;
        let target = Node::start(fixture.directory.path().join("target"))
            .await
            .unwrap();
        fixture
            .node
            .link()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(fixture.node.link().remote(target.link().address()));
        let root = fixture.directory.path().join("target-project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Transfer fixture".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let request = fixture.client.prepare(Command::CreateSessionAt {
            target: target.id(),
            project: Some(project.id),
            worktree: None,
            config: Box::new(config.clone()),
            provider_revision: provider.revision,
        });
        let error = fixture.client.execute(request).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotConfigured, "{api:?}: {error:?}");
        for role in [false, true] {
            let mut bundle = SessionImport {
                source: fixture.node.id(),
                project: Some(project.id),
                worktree: None,
                config: config.clone(),
                provider: provider.clone(),
                secret: None,
                expires_at_ms: None,
                roles: role::Snapshot::default(),
                role_credentials: vec![],
            };
            if role {
                bundle.roles = role::Snapshot {
                    profiles: vec![role::Profile {
                        appearance: None,
                        id: RoleId::new(),
                        revision: 1,
                        key: "review".into(),
                        name: "Review".into(),
                        description: "Review the delegated task".into(),
                        model: Some(role::Model {
                            provider: provider.id,
                            model: "fixture-a".into(),
                            effort: None,
                        }),
                        max_turns: None,
                        skills: vec![],
                        instructions: "Review the task".into(),
                    }],
                    providers: vec![provider.clone()],
                };
                bundle.provider.id = ProviderId::new();
                bundle.provider.authentication = Authentication::ApiKey;
                bundle.config.provider = bundle.provider.id;
            }
            let request = client.prepare(Command::ImportSession(Box::new(bundle)));
            let error = client.execute(request).await.unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::NotConfigured,
                "{api:?}, role={role}: {error:?}"
            );
        }
        assert_eq!(
            execute(&client, Command::ListCredentials).await,
            Output::Credentials(vec![])
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(snapshot.sessions.is_empty());
        target.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
