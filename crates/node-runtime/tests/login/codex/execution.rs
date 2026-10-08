use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn captured(fixture: &Fixture, turn: &QueuedTurn) -> Provider {
    let db = rusqlite::Connection::open_with_flags(
        fixture.node.profile().join("storage/node.sqlite3"),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_capture_through_refresh() {
    for remote in [false, true] {
        let initial = options("0.161.0", "captured");
        let current = options("0.162.0", "current");
        let expected = initial.clone();
        let latest = current.clone();
        let phase = Arc::new(AtomicUsize::new(0));
        let stage = phase.clone();
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            let options = if stage.load(Ordering::SeqCst) == 0 {
                &expected
            } else {
                &latest
            };
            assert_eq!(request.headers["user-agent"], options.user_agent());
            if request.path == "/oauth/token" {
                let index = count.fetch_add(1, Ordering::SeqCst);
                assert!(index < 2, "persisted refresh must not repeat");
                if index == 0 {
                    Reply::Json(server::tokens(request, 30, ACCESS, REFRESH))
                } else {
                    Reply::Json(server::tokens(request, 3600, REFRESHED, ROTATED))
                }
            } else if request.path.starts_with("/models") {
                server::catalog(request, options, ACCESS)
            } else if request.path == "/responses" {
                server::generation(request, options, REFRESHED)
            } else {
                server::standard(request, 30)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = configured(&fixture, Some(initial.clone())).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the isolated file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert_eq!(captured(&fixture, &turn).oauth, Some(initial.clone()));
        let mut changed = provider.clone();
        changed.oauth = Some(current.clone());
        let Output::Provider(changed) = fixture
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
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        assert_eq!(captured(&fixture, &turn).oauth, Some(initial));
        assert_eq!(saved(&fixture.client, provider.id).await, changed);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        let page =
            super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(answer(&page, turn.id), "Authorized Codex response");
        assert!(
            serde_json::to_string(&page)
                .unwrap()
                .contains("Isolated tool content")
        );
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        phase.store(1, Ordering::SeqCst);
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        let db = rusqlite::Connection::open_with_flags(
            fixture.node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let stored: Vec<u8> = db
            .query_row(
                "SELECT body FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        let reference = provider.credential.as_ref().unwrap();
        let grant: serde_json::Value = serde_json::from_str(
            stored["authorizations"][reference.id.to_string()]["key"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(grant["refresh"], ROTATED);
        assert_eq!(grant["access"], REFRESHED);
        drop(db);
        let (_, next) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
        assert_eq!(captured(&fixture, &next).oauth, Some(current));
        assert_eq!(answer(&page, next.id), "Authorized Codex response");
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/responses")
                .count(),
            4
        );
        drop(requests);
        fixture.close().await;
    }
}
