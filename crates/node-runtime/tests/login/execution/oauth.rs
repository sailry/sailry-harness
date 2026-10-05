use super::*;
use sailry_protocol::conversation::oauth::Options;

fn captured(fixture: &Fixture, turn: &QueuedTurn) -> Provider {
    let db = rusqlite::Connection::open_with_flags(
        fixture.directory.path().join("node/storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let body: Vec<u8> = db
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn queued(fixture: &Fixture, session: &Session) -> QueuedTurn {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Authorization fixture".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captures_effective_defaults_without_changing_provider() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = signed_in(&fixture, authentication, api).await;
            let session = session(&fixture, &provider).await;
            let turn = queued(&fixture, &session).await;
            assert_eq!(
                captured(&fixture, &turn).oauth,
                Options::defaults(authentication)
            );
            let Output::Providers(providers) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            assert_eq!(
                providers.iter().find(|value| value.id == provider.id),
                Some(&provider)
            );
            assert_eq!(provider.oauth, None);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keeps_captured_headers_through_restart_and_refresh() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let options = super::super::oauth::options(authentication, "captured");
            let expected = options.clone();
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                assert_eq!(request.headers["user-agent"], expected.user_agent());
                if exchange(request) {
                    if authentication == Authentication::Copilot {
                        super::super::oauth::headers(request, &expected);
                    }
                    let fresh = count.fetch_add(1, Ordering::SeqCst) > 0;
                    Reply::Json(exchanged(authentication, fresh))
                } else if request.path.starts_with("/models") {
                    super::super::oauth::headers(request, &expected);
                    if authentication == Authentication::ChatGpt {
                        assert_eq!(request.path, "/models?client_version=0.161.0");
                    }
                    Reply::Json(catalog::models(authentication))
                } else if matches!(request.path.as_str(), "/responses" | "/chat/completions") {
                    super::super::oauth::headers(request, &expected);
                    generation(request, authentication, true)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = configured(&fixture, authentication, api, Some(options.clone())).await;
            let session = session(&fixture, &provider).await;
            let turn = queued(&fixture, &session).await;
            assert_eq!(captured(&fixture, &turn).oauth, Some(options));
            let mut changed = provider.clone();
            changed.oauth = Some(super::super::oauth::options(authentication, "current"));
            let Output::Provider(saved) = fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider: changed,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let fixture = renewals::restart(fixture, &server, remote).await;
            assert_eq!(
                captured(&fixture, &turn).oauth,
                Some(super::super::oauth::options(authentication, "captured"))
            );
            let Output::Snapshot(snapshot) = fixture
                .client
                .execute(fixture.client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert_eq!(
                snapshot.providers.iter().find(|value| value.id == saved.id),
                Some(&saved)
            );
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StartQueuedTurn { turn: turn.id }),
                )
                .await
                .unwrap();
            finished(&fixture.client, &turn, Status::Completed).await;
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            assert_eq!(server.requests.lock().unwrap().len(), 7);
            fixture.close().await;
        }
    }
}
