use super::*;
use serde_json::{Value, json};

async fn select(current: &mut Fixture, effort: Effort) {
    let mut config = current.session.config.clone();
    config.effort = effort;
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

async fn capability(current: &mut Fixture, enabled: bool) {
    let mut provider = current.provider.clone();
    provider.models[0].efforts = vec![Effort::Low, Effort::High, Effort::Max];
    provider.models[0].capabilities = Some(
        serde_json::from_value(json!({
            "reasoning":true, "configuration_updates":enabled,
        }))
        .unwrap(),
    );
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

async fn complete(current: &Fixture, message: &str) {
    let turn = current.submit(message).await;
    let page = finished(&current.client, current.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
}

async fn summary_server() -> Server {
    completion::server(vec![json!({
        "type":"response.completed", "sequence_number":1,
        "response":{"id":"native-response","object":"response","created_at":0,
            "model":"fixture-a","status":"completed","output":[{
                "type":"message","id":"native-answer","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":crate::agent_support::compaction::handoff(server::ANSWER),"annotations":[]}]
            }]}
    })]).await
}

fn updates(body: &Value) -> Vec<&Value> {
    body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "configuration_update")
        .collect()
}

fn preserves(left: &Value, right: &Value) {
    for name in ["model", "instructions", "tools", "reasoning", "include"] {
        assert_eq!(
            serde_json::to_vec(&left[name]).unwrap(),
            serde_json::to_vec(&right[name]).unwrap(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn preserves_prefix_across_effort_changes() {
    for remote in [false, true] {
        for websocket in [false, true] {
            let server = Server::transport(
                ModelApi::Responses,
                Reply::Text,
                if websocket {
                    server::Mode::Accept
                } else {
                    server::Mode::Refuse(426)
                },
            )
            .await;
            let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            capability(&mut current, true).await;
            current
                .node
                .set_model_transport_fixture(adk_model::openai::WsOptions {
                    enabled: Arc::new(move || websocket),
                    ..Default::default()
                });
            select(&mut current, Effort::Max).await;
            complete(&current, "Original request").await;
            // Settings that were never sent must not create adjacent input updates.
            select(&mut current, Effort::Low).await;
            select(&mut current, Effort::High).await;
            complete(&current, "Changed effort request").await;
            complete(&current, "Same effort request").await;
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 3, "remote={remote}, websocket={websocket}");
            assert!(updates(&requests[0].body).is_empty());
            assert_eq!(
                updates(&requests[1].body),
                [&json!({"type":"configuration_update","reasoning":{"effort":"high"}})]
            );
            for request in &requests {
                assert_eq!(request.body["reasoning"]["effort"], "max");
                assert!(!request.body.to_string().contains("provider_data"));
                assert!(
                    !request
                        .body
                        .to_string()
                        .contains("configuration_update\",\"reasoning\":{\"effort\":\"low")
                );
                preserves(&requests[0].body, &request.body);
            }
            if websocket {
                assert_eq!(requests[1].body["previous_response_id"], "native-response");
                assert_eq!(requests[2].body["previous_response_id"], "native-response");
                assert_eq!(requests[1].body["input"][0]["type"], "configuration_update");
                assert!(updates(&requests[2].body).is_empty());
                assert_eq!(server.upgrades.lock().unwrap().len(), 1);
            } else {
                let first = requests[0].body["input"].as_array().unwrap();
                let second = requests[1].body["input"].as_array().unwrap();
                let third = requests[2].body["input"].as_array().unwrap();
                assert_eq!(
                    serde_json::to_vec(first).unwrap(),
                    serde_json::to_vec(&second[..first.len()]).unwrap()
                );
                assert_eq!(
                    serde_json::to_vec(second).unwrap(),
                    serde_json::to_vec(&third[..second.len()]).unwrap()
                );
                assert_eq!(updates(&requests[2].body), updates(&requests[1].body));
                assert!(
                    requests
                        .iter()
                        .all(|request| request.body.get("previous_response_id").is_none())
                );
            }
            let events = super::stateless::fixture::events(&current.node, current.session.id).await;
            let events: Vec<adk_core::Event> = events
                .iter()
                .map(|body| serde_json::from_slice(body).unwrap())
                .collect();
            let user: Vec<_> = events
                .iter()
                .filter(|event| event.author == "user")
                .collect();
            assert_eq!(user.len(), 3);
            assert_eq!(user[0].provider_metadata["sailry.context.input_part"], "0");
            assert_eq!(user[1].provider_metadata["sailry.context.input_part"], "1");
            assert_eq!(user[2].provider_metadata["sailry.context.input_part"], "0");
            assert_eq!(
                user.iter()
                    .flat_map(|event| &event.content().unwrap().parts)
                    .filter(|part| matches!(
                        part.provider_part(),
                        adk_core::Part::ConfigurationUpdate { .. }
                    ))
                    .count(),
                1
            );
            current.controller.close().await.unwrap();
            current.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn restores_effort_after_restart_and_compaction() {
    for remote in [false, true] {
        let server = summary_server().await;
        let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        capability(&mut current, true).await;
        current
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                enabled: Arc::new(|| false),
                ..Default::default()
            });
        select(&mut current, Effort::Max).await;
        complete(&current, &"Initial request evidence ".repeat(200)).await;
        select(&mut current, Effort::High).await;
        complete(&current, "Updated request").await;
        let profile = current.node.profile().to_owned();
        current.node.shutdown().await.unwrap();
        current.node = Node::start_empty(profile).await.unwrap();
        current
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                enabled: Arc::new(|| false),
                ..Default::default()
            });
        current.client = Client::new(if remote {
            current
                .controller
                .handle()
                .remote(current.node.link().address())
        } else {
            current.node.local()
        });
        complete(&current, "Restored request").await;
        let Output::QueuedTurn(compaction) = execute(
            &current.client,
            Command::CompactContext {
                session: current.session.id,
                expected_revision: current.session.revision,
                focus: None,
            },
        )
        .await
        else {
            panic!("compaction expected")
        };
        let page = finished(&current.client, current.session.id, compaction.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        complete(&current, "Continue the summary").await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        let old = requests[1].body["input"].as_array().unwrap();
        let restored = requests[2].body["input"].as_array().unwrap();
        assert_eq!(
            serde_json::to_vec(old).unwrap(),
            serde_json::to_vec(&restored[..old.len()]).unwrap()
        );
        assert_eq!(updates(&requests[2].body).len(), 1);
        assert_eq!(requests[4].body["input"][0]["type"], "configuration_update");
        assert_eq!(updates(&requests[4].body).len(), 1);
        assert!(
            requests
                .iter()
                .all(|request| request.body["reasoning"]["effort"] == "max")
        );
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn omits_updates_without_model_capability() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Responses, Reply::Text).await;
        let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        current
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                enabled: Arc::new(|| false),
                ..Default::default()
            });
        capability(&mut current, true).await;
        select(&mut current, Effort::Max).await;
        complete(&current, &"Initial request evidence ".repeat(200)).await;
        select(&mut current, Effort::High).await;
        complete(&current, "Updated request").await;
        capability(&mut current, false).await;
        select(&mut current, Effort::Low).await;
        complete(&current, "Unsupported request").await;
        capability(&mut current, true).await;
        select(&mut current, Effort::High).await;
        complete(&current, "Restore supported request").await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2].body["reasoning"]["effort"], "low");
        assert!(updates(&requests[2].body).is_empty());
        assert_eq!(requests[3].body["reasoning"]["effort"], "max");
        assert_eq!(updates(&requests[3].body).len(), 1);
        assert_eq!(updates(&requests[3].body)[0]["reasoning"]["effort"], "high");
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn restores_owner_after_unsupported_compaction() {
    for remote in [false, true] {
        let server = summary_server().await;
        let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        capability(&mut current, true).await;
        let mut other = current.provider.models[0].clone();
        other.id = "fixture-b".into();
        other.capabilities.as_mut().unwrap().configuration_updates = Some(false);
        current.provider.models.push(other);
        let Output::Provider(provider) = execute(
            &current.client,
            Command::PutProvider {
                expected_revision: current.provider.revision,
                provider: current.provider.clone(),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        current.provider = provider;
        current
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                enabled: Arc::new(|| false),
                ..Default::default()
            });
        select(&mut current, Effort::Max).await;
        complete(&current, &"Initial owner request evidence ".repeat(200)).await;
        select(&mut current, Effort::High).await;
        complete(&current, "Changed owner request").await;
        let mut config = current.session.config.clone();
        config.model = "fixture-b".into();
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
        let Output::QueuedTurn(compaction) = execute(
            &current.client,
            Command::CompactContext {
                session: current.session.id,
                expected_revision: current.session.revision,
                focus: None,
            },
        )
        .await
        else {
            panic!("compaction expected")
        };
        let page = finished(&current.client, current.session.id, compaction.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let mut config = current.session.config.clone();
        config.model = "fixture-a".into();
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
        complete(&current, "Restore the original owner").await;
        complete(&current, "Keep the restored setting").await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(updates(&requests[2].body).is_empty());
        assert_eq!(updates(&requests[3].body).len(), 1);
        assert_eq!(updates(&requests[4].body).len(), 1);
        assert_eq!(updates(&requests[3].body)[0]["reasoning"]["effort"], "high");
        assert_eq!(requests[3].body["reasoning"]["effort"], "max");
        let restored = requests[3].body["input"].as_array().unwrap();
        let next = requests[4].body["input"].as_array().unwrap();
        assert_eq!(
            serde_json::to_vec(restored).unwrap(),
            serde_json::to_vec(&next[..restored.len()]).unwrap()
        );
        let events = super::stateless::fixture::events(&current.node, current.session.id).await;
        let events: Vec<adk_core::Event> = events
            .iter()
            .map(|body| serde_json::from_slice(body).unwrap())
            .collect();
        let user: Vec<_> = events
            .iter()
            .filter(|event| event.author == "user")
            .rev()
            .take(2)
            .collect();
        assert!(
            !user[0].content().unwrap().parts.iter().any(|part| matches!(
                part.provider_part(),
                adk_core::Part::ConfigurationUpdate { .. }
            ))
        );
        assert_eq!(
            user[1]
                .content()
                .unwrap()
                .parts
                .iter()
                .filter(|part| matches!(
                    part.provider_part(),
                    adk_core::Part::ConfigurationUpdate { .. }
                ))
                .count(),
            1
        );
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}
