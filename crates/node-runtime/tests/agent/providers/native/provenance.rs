use super::stateless::fixture;
use super::*;
use serde_json::Value;

fn reply(offset: usize) -> Reply {
    Reply::Dynamic(Arc::new(move |index| server::Script {
        bytes: fixture::stream(offset + index),
        ending: server::Ending::Close,
    }))
}

async fn complete(current: &Fixture) {
    let turn = current.submit("Continue the same conversation").await;
    let page = finished(&current.client, current.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
    let public = serde_json::to_string(&page).unwrap();
    for secret in [
        server::KEY,
        "replacement-private-key",
        "rotated-private-key",
    ] {
        assert!(!public.contains(secret));
    }
}

fn encrypted(body: &Value) -> Vec<String> {
    assert!(!body.to_string().contains("provider_data"));
    assert!(!body.to_string().contains("provider_origin"));
    body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["encrypted_content"].as_str().map(str::to_owned))
        .collect()
}

async fn save(current: &mut Fixture, provider: Provider) {
    let Output::Provider(provider) = execute(
        &current.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    current.provider = provider;
}

async fn select(current: &mut Fixture, model: &str) {
    let mut config = current.session.config.clone();
    config.model = model.into();
    let Output::Session(session) = execute(
        &current.client,
        Command::SetSessionConfig {
            session: current.session.id,
            expected_revision: current.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    current.session = session;
}

#[tokio::test]
async fn isolates_account_endpoint_model_and_protocol() {
    for remote in [false, true] {
        let first = Server::start(ModelApi::Responses, reply(1)).await;
        let second = Server::start(ModelApi::Responses, reply(101)).await;
        let gemini = Server::start(ModelApi::Gemini, Reply::Text).await;
        let mut current = Fixture::empty(remote, ModelApi::Responses, &first.endpoint).await;
        complete(&current).await;
        let original = fixture::events(current.node.profile(), current.session.id);
        complete(&current).await;
        assert_eq!(
            encrypted(&first.requests.lock().unwrap()[1].body),
            ["sealed 1 +/ opaque"]
        );
        let reference = current.session.config.credential.as_ref().unwrap();
        let Output::Credentials(credentials) =
            execute(&current.client, Command::ListCredentials).await
        else {
            panic!("credentials expected")
        };
        let credential = credentials
            .iter()
            .find(|credential| credential.id == reference.id)
            .unwrap();
        let Output::Credential(other) = execute(
            &current.client,
            Command::PutCredential {
                id: CredentialId::new(),
                provider: current.provider.id,
                expected_revision: 0,
                secret: Secret::new("replacement-private-key".into()),
                expires_at_ms: None,
            },
        )
        .await
        else {
            panic!("credential expected")
        };
        assert_eq!(other.revision, credential.revision);
        assert_ne!(other.id, credential.id);
        let mut config = current.session.config.clone();
        config.credential.as_mut().unwrap().id = other.id;
        let Output::Session(session) = execute(
            &current.client,
            Command::SetSessionConfig {
                session: current.session.id,
                expected_revision: current.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        current.session = session;
        complete(&current).await;
        assert!(encrypted(&first.requests.lock().unwrap()[2].body).is_empty());
        complete(&current).await;
        assert_eq!(
            encrypted(&first.requests.lock().unwrap()[3].body),
            ["sealed 3 +/ opaque"]
        );
        assert_eq!(
            first.requests.lock().unwrap()[2].headers["authorization"],
            "Bearer replacement-private-key"
        );
        let output = execute(
            &current.client,
            Command::PutCredential {
                id: other.id,
                provider: current.provider.id,
                expected_revision: other.revision,
                secret: Secret::new("rotated-private-key".into()),
                expires_at_ms: None,
            },
        )
        .await;
        assert!(matches!(output, Output::Credential(_)));
        complete(&current).await;
        assert!(encrypted(&first.requests.lock().unwrap()[4].body).is_empty());
        assert_eq!(
            first.requests.lock().unwrap()[4].headers["authorization"],
            "Bearer rotated-private-key"
        );
        let mut provider = current.provider.clone();
        provider.endpoint = second.endpoint.clone();
        save(&mut current, provider).await;
        complete(&current).await;
        assert!(encrypted(&second.requests.lock().unwrap()[0].body).is_empty());
        let mut provider = current.provider.clone();
        let mut model = provider.models[0].clone();
        model.id = "fixture-b".into();
        provider.models.push(model);
        save(&mut current, provider).await;
        select(&mut current, "fixture-b").await;
        complete(&current).await;
        assert!(encrypted(&second.requests.lock().unwrap()[1].body).is_empty());
        let mut provider = current.provider.clone();
        provider.api = ModelApi::Gemini;
        provider.endpoint = gemini.endpoint.clone();
        save(&mut current, provider).await;
        complete(&current).await;
        {
            let requests = gemini.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let body = requests[0].body.to_string();
            assert!(body.contains("Stateless answer 1") && body.contains("Stateless answer 102"));
            for forbidden in [
                "encrypted_content",
                "sealed ",
                "provider_data",
                "provider_origin",
                "toolCall",
                "toolResponse",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "remote={remote}, {forbidden}: {body}"
                );
            }
        }
        let events = fixture::events(current.node.profile(), current.session.id);
        assert_eq!(&events[..original.len()], &original);
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn omits_originless_history() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Responses, reply(1)).await;
        let current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        complete(&current).await;
        // Reproduce a v1 event written before provider ownership existed. Only
        // this isolated fixture is changed; the product never rewrites history.
        let database =
            rusqlite::Connection::open(current.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let rows = database
            .prepare("SELECT rowid,body FROM agent_events WHERE session=?1 ORDER BY rowid")
            .unwrap()
            .query_map([current.session.id.to_string()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut changed = 0;
        for (row, bytes) in rows {
            let mut event: Value = serde_json::from_slice(&bytes).unwrap();
            if let Some(metadata) = event
                .pointer_mut("/provider_metadata")
                .and_then(Value::as_object_mut)
                && metadata.remove("provider_origin").is_some()
            {
                database
                    .execute(
                        "UPDATE agent_events SET body=?1 WHERE rowid=?2",
                        rusqlite::params![serde_json::to_vec(&event).unwrap(), row],
                    )
                    .unwrap();
                changed += 1;
            }
        }
        assert!(changed > 0);
        drop(database);
        let original = fixture::events(current.node.profile(), current.session.id);
        complete(&current).await;
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(encrypted(&requests[1].body).is_empty());
            assert!(requests[1].body.to_string().contains("Stateless answer 1"));
        }
        let events = fixture::events(current.node.profile(), current.session.id);
        assert_eq!(&events[..original.len()], &original);
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}
