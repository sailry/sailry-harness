use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[path = "../../support/reasoning.rs"]
mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_only_encrypted_history() {
    for remote in [false, true] {
        let count = Arc::new(AtomicUsize::new(0));
        let generation = count.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let bodies = requests.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/oauth/token" {
                // The same verified account and access bytes survive a real refresh.
                return Reply::Json(server::tokens(request, 3600, ACCESS, REFRESH));
            }
            if request.path != "/responses" {
                return server::standard(request, 3600);
            }
            if request.method == "GET" {
                return Reply::Raw("HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
            }
            let body = server::input(
                request,
                &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
                ACCESS,
            );
            let index = generation.fetch_add(1, Ordering::SeqCst);
            fixture::payload(&body, index);
            bodies.lock().unwrap().push(body);
            server::stream(vec![server::completed(fixture::output(index))], true)
        })
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let mut provider = configured(&fixture, None).await;
        provider.models[0].reasoning = true;
        provider.models[0].capabilities =
            Some(serde_json::from_value(serde_json::json!({"reasoning":true})).unwrap());
        provider.models[0].efforts = vec![Effort::Disabled];
        provider.models[0].default_effort = Effort::Disabled;
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let session = super::super::execution::session(&fixture, &provider).await;
        let before = super::transport::credential(&fixture, &provider).await;
        let mut original = Vec::new();
        for index in 0..4 {
            if index == 2 {
                super::transport::expire(&fixture, &provider);
            }
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            let page =
                super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
            assert!(answer(&page, turn.id).contains(&format!("Stateless answer {index}")));
            assert_eq!(requests.lock().unwrap().len(), index + 1);
            assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part| {
                matches!(part, Part::Thinking(value) if value.contains("Legacy summary without encrypted content"))
            }));
            let events = fixture::events(fixture.node.profile(), session.id);
            if index == 0 {
                assert!(
                    events
                        .iter()
                        .any(|body| String::from_utf8_lossy(body).contains("stateless-0"))
                );
                original = events;
            } else {
                assert_eq!(&events[..original.len()], &original);
            }
        }
        let after = super::transport::credential(&fixture, &provider).await;
        assert_eq!(after.id, before.id);
        assert!(after.revision > before.revision);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn isolates_verified_accounts_and_endpoints() {
    use super::transport::native;
    use std::sync::atomic::AtomicBool;
    for remote in [false, true] {
        let changed = Arc::new(AtomicBool::new(false));
        let account = changed.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/oauth/token" && account.load(Ordering::SeqCst) {
                let mut tokens = server::tokens(request, 3600, "other-private-access", REFRESH);
                tokens["id_token"] = json!(super::super::identity::signed(&json!({
                    "iss":super::super::identity::issuer(request),"aud":CLIENT,
                    "sub":"other-private-subject","iat":future_expiry()-3600,"exp":future_expiry(),
                    "https://api.openai.com/auth":{"chatgpt_account_id":"other-private-account"}
                })));
                return Reply::Json(tokens);
            }
            if request.path.starts_with("/models") && account.load(Ordering::SeqCst) {
                assert_eq!(
                    request.headers["authorization"],
                    "Bearer other-private-access"
                );
                assert_eq!(
                    request.headers["chatgpt-account-id"],
                    "other-private-account"
                );
                return Reply::Json(server::models());
            }
            server::standard(request, 3600)
        })
        .await;
        let response = |offset| {
            native::Reply::Dynamic(Arc::new(move |index| native::Script {
                bytes: fixture::stream(offset + index),
                ending: native::Ending::Close,
            }))
        };
        let first = native::Server::start(ModelApi::Responses, response(1)).await;
        let second = native::Server::start(ModelApi::Responses, response(101)).await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, None).await;
        fixture.node.set_model_endpoint_fixture(&first.endpoint);
        let mut session = super::super::execution::session(&fixture, &provider).await;
        let complete = async |session: &Session| {
            let (_, turn) = super::super::execution::submit(&fixture.client, session).await;
            let page =
                super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
            no_secrets(&page);
            for private in [
                "other-private-subject",
                "other-private-account",
                "other-private-access",
            ] {
                assert!(!serde_json::to_string(&page).unwrap().contains(private));
            }
        };
        complete(&session).await;
        let original = fixture::events(fixture.node.profile(), session.id);
        complete(&session).await;
        let opaque = |body: &serde_json::Value| {
            body["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["encrypted_content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            opaque(&first.requests.lock().unwrap()[1].body),
            ["sealed 1 +/ opaque"]
        );
        changed.store(true, Ordering::SeqCst);
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
        drop(updates);
        let current = saved(&fixture.client, provider.id).await;
        let Output::Session(updated) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadSession {
                session: session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(updated.revision, session.revision + 1);
        assert_eq!(updated.config.credential, current.credential);
        session = updated;
        complete(&session).await;
        {
            let requests = first.requests.lock().unwrap();
            assert!(opaque(&requests[2].body).is_empty());
            assert_eq!(
                requests[2].headers["authorization"],
                "Bearer other-private-access"
            );
            assert_eq!(
                requests[2].headers["chatgpt-account-id"],
                "other-private-account"
            );
            assert!(requests[2].body.to_string().contains("Stateless answer 1"));
        }
        complete(&session).await;
        assert_eq!(
            opaque(&first.requests.lock().unwrap()[3].body),
            ["sealed 3 +/ opaque"]
        );
        fixture.node.set_model_endpoint_fixture(&second.endpoint);
        complete(&session).await;
        {
            let requests = second.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(opaque(&requests[0].body).is_empty());
            assert_eq!(
                requests[0].headers["chatgpt-account-id"],
                "other-private-account"
            );
            assert!(!requests[0].body.to_string().contains("provider_origin"));
        }
        let events = fixture::events(fixture.node.profile(), session.id);
        assert_eq!(&events[..original.len()], &original);
        fixture.close().await;
    }
}
