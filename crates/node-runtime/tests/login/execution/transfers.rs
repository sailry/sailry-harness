use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_rotating_grant_copies() {
    let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
    let fixture = Fixture::new(false, &server).await;
    let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
    let source_session = session(&fixture, &provider).await;
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
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Transfer fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let request = fixture.client.prepare(Command::CreateSessionAt {
        target: target.id(),
        project: Some(project.id),
        worktree: None,
        config: Box::new(source_session.config.clone()),
        provider_revision: provider.revision,
    });
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::NotConfigured
    );
    let grant = Secret::new(json!({"kind":"chat_gpt","access":access_token(),"refresh":REFRESH,"account_id":"fixture-account","expires_at_ms":future_expiry()*1000,"renewal_pending":false}).to_string());
    for role in [false, true] {
        let mut bundle = SessionImport {
            source: fixture.node.id(),
            project: Some(project.id),
            worktree: None,
            config: source_session.config.clone(),
            provider: provider.clone(),
            secret: Some(grant.clone()),
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
                        model: "fixture".into(),
                        effort: None,
                    }),
                    max_turns: None,
                    skills: vec![],
                    instructions: "Review the task".into(),
                }],
                providers: vec![provider.clone()],
            };
            bundle.role_credentials = vec![role::ProviderSecret {
                provider: provider.id,
                secret: grant.clone(),
                expires_at_ms: None,
            }];
            bundle.provider.id = ProviderId::new();
            bundle.provider.authentication = Authentication::ApiKey;
            bundle.provider.credential = None;
            bundle.provider.endpoint = server.endpoint.clone();
            bundle.config.provider = bundle.provider.id;
            bundle.config.credential = None;
            bundle.secret = None;
        }
        let error = client
            .execute(client.prepare(Command::ImportSession(Box::new(bundle))))
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::NotConfigured,
            "role={role}: {error:?}"
        );
        no_secrets(&error);
        assert_eq!(
            client
                .execute(client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            Output::Credentials(vec![])
        );
    }
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    target.shutdown().await.unwrap();
    fixture.close().await;
}
