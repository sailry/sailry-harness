use super::*;
use serde_json::json;

#[tokio::test]
async fn preserves_selected_reasoning() {
    for remote in [false, true] {
        for api in [ModelApi::OpenCodeGo, ModelApi::OpenCodeZen] {
            for model in ["glm-5.1", "deepseek-v4-pro"] {
                let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
                let mut fixture = Fixture::empty(remote, api, &server.endpoint).await;
                fixture.provider.models[0].id = model.into();
                fixture.provider.models[0].efforts =
                    vec![Effort::Disabled, Effort::High, Effort::Max];
                fixture.provider.default_model = model.into();
                fixture.provider.binding =
                    Some(Box::new(sailry_protocol::conversation::presets::Binding {
                        policies: vec![sailry_protocol::conversation::presets::RequestPolicy {
                            models: vec![model.into()],
                            reasoning_replay: None,
                            body: json!({"thinking":{"type":"enabled","clear_thinking":false}})
                                .as_object()
                                .unwrap()
                                .clone(),
                        }],
                        ..Default::default()
                    }));
                execute(
                    &fixture.client,
                    Command::PutProvider {
                        expected_revision: fixture.provider.revision,
                        provider: fixture.provider.clone(),
                    },
                )
                .await;
                for effort in [Effort::Disabled, Effort::High, Effort::Max] {
                    let mut config = fixture.session.config.clone();
                    config.model = model.into();
                    config.effort = effort;
                    let Output::Session(session) = execute(
                        &fixture.client,
                        Command::SetSessionConfig {
                            session: fixture.session.id,
                            expected_revision: fixture.session.revision,
                            config,
                        },
                    )
                    .await
                    else {
                        panic!("session expected")
                    };
                    fixture.session = session;
                    let turn = fixture.submit("Use the captured reasoning settings").await;
                    let page = finished(&fixture.client, fixture.session.id, turn).await;
                    assert_eq!(
                        page.runs.last().unwrap().status,
                        Status::Completed,
                        "{api:?}, {model}, remote={remote}: {:?}",
                        page.runs
                    );
                }
                let requests = server.requests.lock().unwrap().clone();
                assert_eq!(requests.len(), 3);
                for (request, expected) in requests.iter().zip(["none", "high", "max"]) {
                    assert_eq!(
                        request.body["reasoning_effort"], expected,
                        "{api:?}, {model}, remote={remote}"
                    );
                    assert_eq!(request.body["thinking"]["clear_thinking"], false);
                    assert_eq!(
                        request.body["thinking"]["type"],
                        if expected == "none" {
                            "disabled"
                        } else {
                            "enabled"
                        }
                    );
                }
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn retains_errors_and_status() {
    for remote in [false, true] {
        for status in [400, 401, 403] {
            let body =
                json!({"error":{"message":format!("Provider rejected input {}", server::KEY)}})
                    .to_string();
            let reply = format!(
                "HTTP/1.1 {status} Failure\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let server = Server::start(
                ModelApi::ChatCompletions,
                Reply::Raw(Arc::new(reply.into_bytes())),
            )
            .await;
            let mut fixture = Fixture::empty(remote, ModelApi::OpenCodeGo, &server.endpoint).await;
            let model = "deepseek-v4.1-flash";
            fixture.provider.models[0].id = model.into();
            fixture.provider.default_model = model.into();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await;
            let mut config = fixture.session.config.clone();
            config.model = model.into();
            let Output::Session(session) = execute(
                &fixture.client,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            fixture.session = session;
            let turn = fixture.submit("Isolated failure").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            let fault = run.error.as_ref().unwrap();
            assert!(
                fault.message.contains("Provider rejected input"),
                "{fault:?}"
            );
            assert!(fault.message.contains(&status.to_string()), "{fault:?}");
            assert!(fault.message.contains("[redacted]"), "{fault:?}");
            assert_eq!(
                fault.details,
                Some(FaultDetails::ModelRequest {
                    reason: if status == 400 {
                        sailry_protocol::ModelFailure::Request
                    } else {
                        sailry_protocol::ModelFailure::Unauthorized
                    }
                })
            );
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            let attempts = if status == 400 {
                1 + goose_provider_types::retry::DEFAULT_MAX_RETRIES
            } else {
                1
            };
            assert_eq!(server.requests.lock().unwrap().len(), attempts);
            let statistics =
                crate::observations::statistics(&fixture.client, fixture.session.id).await;
            let requests: Vec<_> = statistics
                .observations
                .recent
                .iter()
                .filter(|request| request.turn == turn)
                .collect();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].attempts.len(), 1);
            assert_eq!(requests[0].attempts[0].status, None);
            assert_eq!(
                requests[0].attempts[0].outcome,
                sailry_protocol::conversation::observation::Outcome::Failed
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn local_and_remote_nodes_share_routing_identity() {
    for remote in [false, true] {
        for (api, model, wire, path) in [
            (
                ModelApi::OpenCodeGo,
                "deepseek-v4.1-flash",
                ModelApi::DeepSeek,
                "/v1/chat/completions",
            ),
            (
                ModelApi::OpenCodeGo,
                "minimax-m3",
                ModelApi::ChatCompletions,
                "/v1/chat/completions",
            ),
            (
                ModelApi::OpenCodeGo,
                "gpt-6-luna",
                ModelApi::Responses,
                "/v1/responses",
            ),
            (
                ModelApi::OpenCodeZen,
                "minimax-m3",
                ModelApi::DeepSeek,
                "/v1/chat/completions",
            ),
            (
                ModelApi::OpenCodeZen,
                "qwen3.8-flash",
                ModelApi::ChatCompletions,
                "/v1/chat/completions",
            ),
            (
                ModelApi::OpenCodeZen,
                "gpt-6-luna",
                ModelApi::Responses,
                "/v1/responses",
            ),
            (
                ModelApi::OpenCodeZen,
                "gemini-3.8-flash",
                ModelApi::ChatCompletions,
                "/v1/chat/completions",
            ),
        ] {
            let reply = if wire == ModelApi::Responses {
                let delta = json!({"type":"response.output_text.delta","sequence_number":0,"item_id":"msg_test","output_index":0,"content_index":0,"delta":server::ANSWER,"logprobs":[]});
                let done = json!({"type":"response.completed","sequence_number":1,"response":{
                    "id":"resp_test","object":"response","created_at":1,"model":model,"status":"completed",
                    "output":[{"id":"msg_test","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":server::ANSWER,"annotations":[]}]}],
                    "usage":{"input_tokens":8,"output_tokens":4,"total_tokens":12,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}
                }});
                Reply::Raw(Arc::new(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {delta}\n\ndata: {done}\n\n").into_bytes()))
            } else {
                Reply::Text
            };
            let server = Server::start(wire, reply).await;
            let endpoint = if wire == ModelApi::Anthropic {
                format!("{}/v1", server.endpoint)
            } else {
                server.endpoint.clone()
            };
            let mut fixture = Fixture::empty(remote, api, &endpoint).await;
            fixture.provider.models[0].id = model.into();
            fixture.provider.models[0].context = 65536;
            fixture.provider.models[0].output = 8192;
            fixture.provider.default_model = model.into();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await;
            let mut config = fixture.session.config.clone();
            config.model = model.into();
            let Output::Session(session) = execute(
                &fixture.client,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            fixture.session = session;
            let input =
                "Inspect the project and explain the file changes in a concise report. ".repeat(10);
            let turn = fixture.submit(&input).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            let turn = fixture.submit("Continue with a short summary").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let requests = server.requests.lock().unwrap().clone();
            assert!(requests.len() >= 2);
            for request in requests {
                assert_eq!(request.path, path);
                if wire != ModelApi::Gemini {
                    assert_eq!(request.body["model"], model);
                }
                if api == ModelApi::OpenCodeGo {
                    assert_eq!(
                        request.headers["x-opencode-session"],
                        fixture.session.id.to_string()
                    );
                } else {
                    assert!(!request.headers.contains_key("x-opencode-session"));
                }
                assert!(request.headers["user-agent"].starts_with("Sailry/"));
                let auth = if wire == ModelApi::Anthropic {
                    "x-api-key"
                } else if wire == ModelApi::Gemini {
                    "x-goog-api-key"
                } else {
                    "authorization"
                };
                assert!(request.headers[auth].contains(server::KEY));
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[path = "opencode/live.rs"]
mod live;
