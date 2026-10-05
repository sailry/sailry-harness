use super::*;
use serde_json::json;

#[tokio::test]
async fn keeps_public_status_without_provider_content() {
    for remote in [false, true] {
        for (status, category) in [
            (400, "invalid_input"),
            (401, "unauthorized"),
            (403, "forbidden"),
        ] {
            let body = format!("private provider response {}", server::KEY);
            let reply = format!(
                "HTTP/1.1 {status} Failure\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let server =
                Server::start(ModelApi::DeepSeek, Reply::Raw(Arc::new(reply.into_bytes()))).await;
            let mut fixture = Fixture::new(remote, ModelApi::OpenCodeGo, &server.endpoint).await;
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
            assert_eq!(fault.code, ErrorCode::Unavailable);
            assert!(fault.message.contains(&format!("HTTP {status}")));
            assert!(fault.message.contains(category));
            assert!(!fault.message.contains("private"));
            assert!(!fault.message.contains(server::KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
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
                ModelApi::Anthropic,
                "/v1/messages",
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
                ModelApi::Anthropic,
                "/v1/messages",
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
                ModelApi::Gemini,
                "/v1/models/gemini-3.8-flash:streamGenerateContent?alt=sse",
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
            let mut fixture = Fixture::new(remote, api, &endpoint).await;
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
                assert_eq!(
                    request.headers["x-opencode-session"],
                    fixture.session.id.to_string()
                );
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
