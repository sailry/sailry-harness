use super::*;

#[tokio::test]
async fn shutdown_cancels_queries() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let server = Server::start(ModelApi::Responses, |_| Reply::Hold).await;
        let request = fixture
            .client
            .prepare(discover(source(ModelApi::Responses, &server)));
        let client = fixture.client.clone();
        let query = tokio::spawn(async move { client.execute(request).await });
        server.wait_requests(1).await;
        tokio::time::timeout(Duration::from_secs(5), fixture.node.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), query)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        server.wait_closed(1).await;
        fixture.controller.close().await.unwrap();
    }
}

async fn saved(fixture: &Fixture, server: &Server) -> Provider {
    let Output::Provider(provider) = execute(
        &fixture.client,
        Command::SaveProvider {
            provider: Provider {
                oauth: None,
                options: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Saved discovery fixture".into(),
                api: ModelApi::Responses,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: server.endpoint.clone(),
                enabled: true,
                models: vec![],
                default_model: String::new(),
                credential: None,
            },
            expected_revision: 0,
            secret: Some(Secret::new(KEY.into())),
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    provider
}

#[tokio::test]
async fn rejects_stale_configuration() {
    for remote in [false, true] {
        for revoke in [false, true] {
            let fixture = Fixture::new(remote).await;
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let server = Server::start(ModelApi::Responses, move |_| {
                Reply::Delayed(gate.clone(), json!({"data":[]}))
            })
            .await;
            let provider = saved(&fixture, &server).await;
            let client = fixture.client.clone();
            let request = client.prepare(Command::ValidateProvider {
                provider: provider.id,
                expected_revision: provider.revision,
            });
            let job = tokio::spawn(async move { client.execute(request).await });
            server.wait_requests(1).await;
            if revoke {
                execute(
                    &fixture.client,
                    Command::RevokeCredential {
                        id: provider.credential.as_ref().unwrap().id,
                        expected_revision: 1,
                    },
                )
                .await;
            } else {
                let mut changed = provider.clone();
                changed.name = "Changed while discovering".into();
                execute(
                    &fixture.client,
                    Command::PutProvider {
                        expected_revision: changed.revision,
                        provider: changed,
                    },
                )
                .await;
            }
            release.notify_one();
            let error = job.await.unwrap().unwrap_err();
            assert_eq!(
                error.code,
                if revoke {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn validates_before_network_access() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let server = Server::start(ModelApi::Responses, |_| Reply::Json(json!({"data":[]}))).await;
        let provider = saved(&fixture, &server).await;
        let mut input = source(ModelApi::Responses, &server);
        input.secret = None;
        input.credential = provider.credential.clone();
        let error = fixture
            .client
            .execute(fixture.client.prepare(discover(input.clone())))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        input.provider = provider.id;
        input.credential.as_mut().unwrap().node = NodeId([7; 32]);
        let error = fixture
            .client
            .execute(fixture.client.prepare(discover(input)))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        for endpoint in [
            format!("{}?key=not-allowed", server.endpoint),
            "file:///tmp/provider".into(),
            "https://user:password@example.test".into(),
        ] {
            let mut input = source(ModelApi::Responses, &server);
            input.endpoint = endpoint;
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(discover(input)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}
