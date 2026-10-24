use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
#[expect(
    dead_code,
    reason = "The shared native provider fixture includes other protocol response modes"
)]
#[path = "../../agent/providers/native/server.rs"]
pub(super) mod native;

pub(super) async fn expire(fixture: &Fixture, provider: &Provider) {
    let bytes: Vec<u8> = {
        let sql = ("SELECT body FROM providers WHERE id=?1").to_owned();
        let parameters = turso::params![provider.id.to_string()];
        fixture
            .node
            .with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let value: turso::Result<_> = row.get(0);
                    value.map_err(crate::database_fixture::fault)
                })
            })
            .await
    }
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
        {
            let sql = ("UPDATE providers SET body=?1 WHERE id=?2").to_owned();
            let parameters =
                turso::params![serde_json::to_vec(&body).unwrap(), provider.id.to_string()];
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        db.execute(sql, parameters)
                            .await
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
        }
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
async fn reconnects_only_after_headers_change() {
    const RENEWED: &str = "renewed-native-access";
    for remote in [false, true] {
        for same_token in [false, true] {
            let exchanges = Arc::new(AtomicUsize::new(0));
            let issued = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/oauth/token" {
                    let index = issued.fetch_add(1, Ordering::SeqCst);
                    let access = if index == 0 || same_token {
                        ACCESS
                    } else {
                        RENEWED
                    };
                    return Reply::Json(server::tokens(request, 3600, access, REFRESH));
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
            expire(&fixture, &provider).await;
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
            assert_eq!(upgrades.len(), if same_token { 1 } else { 2 });
            assert_eq!(
                upgrades[0].headers["authorization"],
                format!("Bearer {ACCESS}")
            );
            if !same_token {
                assert_eq!(
                    upgrades[1].headers["authorization"],
                    format!("Bearer {RENEWED}")
                );
                for name in [
                    "chatgpt-account-id",
                    "session-id",
                    "user-agent",
                    "version",
                    "originator",
                    "openai-beta",
                ] {
                    assert_eq!(upgrades[0].headers.get(name), upgrades[1].headers.get(name));
                }
            }
            let requests = model.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            assert!(
                requests
                    .iter()
                    .all(|request| request.body["type"] == "response.create")
            );
            assert!(requests[0].body.get("previous_response_id").is_none());
            if same_token {
                assert_eq!(requests[1].body["previous_response_id"], "native-response");
            } else {
                assert!(requests[1].body.get("previous_response_id").is_none());
            }
            no_secrets(&page);
            assert!(!serde_json::to_string(&page).unwrap().contains(RENEWED));
            fixture.close().await;
        }
    }
}
