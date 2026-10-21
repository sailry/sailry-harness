use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
#[expect(
    dead_code,
    reason = "The shared native provider fixture includes other protocol response modes"
)]
#[path = "../../agent/providers/native/server.rs"]
pub(super) mod native;

pub(super) fn expire(fixture: &Fixture, provider: &Provider) {
    let db =
        rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3")).unwrap();
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT body FROM providers WHERE id=?1",
            [provider.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let mut body: Value = serde_json::from_slice(&bytes).unwrap();
    let reference = provider.credential.as_ref().unwrap();
    let stored = &mut body["authorizations"][reference.id.to_string()];
    let mut grant: Value = serde_json::from_str(stored["key"].as_str().unwrap()).unwrap();
    assert_eq!(grant["kind"], "chat_gpt_oauth");
    // Expire only this isolated fixture's access token; the next public turn
    // must use the ordinary Node refresh owner and its credential revision.
    grant["expires_at_ms"] = json!(1);
    stored["key"] = json!(grant.to_string());
    assert_eq!(
        db.execute(
            "UPDATE providers SET body=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_vec(&body).unwrap(), provider.id.to_string()]
        )
        .unwrap(),
        1
    );
}

pub(super) async fn credential(fixture: &Fixture, provider: &Provider) -> Credential {
    let Output::Credentials(values) = fixture
        .client
        .execute(fixture.client.prepare(Command::ListCredentials))
        .await
        .unwrap()
    else {
        panic!("credentials expected");
    };
    values
        .into_iter()
        .find(|value| value.id == provider.credential.as_ref().unwrap().id)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnects_after_oauth_revision() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let issued = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/oauth/token" {
                issued.fetch_add(1, Ordering::SeqCst);
                // Equal access bytes make the credential revision, rather than
                // a changed authorization header, the reconnect reason.
                return Reply::Json(server::tokens(request, 3600, ACCESS, REFRESH));
            }
            server::standard(request, 3600)
        })
        .await;
        let model = native::Server::transport(
            ModelApi::Responses,
            native::Reply::Text,
            native::Mode::Accept,
        )
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, None).await;
        fixture.node.set_model_endpoint_fixture(&model.endpoint);
        let session = super::super::execution::session(&fixture, &provider).await;
        let before = credential(&fixture, &provider).await;
        let (_, first) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &first, Status::Completed).await;
        assert_eq!(answer(&page, first.id), native::ANSWER);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(model.upgrades.lock().unwrap().len(), 1);
        expire(&fixture, &provider);
        let (_, next) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
        assert_eq!(answer(&page, next.id), native::ANSWER);
        let after = credential(&fixture, &provider).await;
        assert_eq!(after.id, before.id);
        assert!(after.revision > before.revision);
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        let upgrades = model.upgrades.lock().unwrap().clone();
        assert_eq!(upgrades.len(), 2);
        for name in [
            "authorization",
            "chatgpt-account-id",
            "session-id",
            "user-agent",
            "version",
            "originator",
            "openai-beta",
        ] {
            assert_eq!(upgrades[0].headers.get(name), upgrades[1].headers.get(name));
        }
        assert_eq!(
            upgrades[0].headers["authorization"],
            format!("Bearer {ACCESS}")
        );
        let requests = model.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.body["type"] == "response.create"
                    && request.body.get("previous_response_id").is_none())
        );
        no_secrets(&page);
        fixture.close().await;
    }
}
