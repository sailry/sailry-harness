use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_rejection_without_replay() {
    for remote in [false, true] {
        let rotations = Arc::new(AtomicUsize::new(0));
        let called = rotations.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| match request
            .path
            .as_str()
        {
            "/token" => tokens(false),
            "/refresh" => {
                called.fetch_add(1, Ordering::SeqCst);
                Reply::Json(json!({"error":"invalid_grant"}))
            }
            "/models" => Reply::Json(catalog::models(Authentication::Plugin)),
            _ => panic!("unexpected fixture request"),
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, true).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        let provider = saved(&fixture.client, provider.id).await;
        for _ in 0..2 {
            let error = fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err();
            private(&error);
            assert_eq!(error.code, ErrorCode::PermissionDenied);
        }
        assert_eq!(rotations.load(Ordering::SeqCst), 1);
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        assert_eq!(
            saved(&fixture.client, provider.id).await.credential,
            provider.credential
        );
        fixture.close().await;
    }
}
