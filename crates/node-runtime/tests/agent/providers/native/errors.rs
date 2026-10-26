use super::*;
use serde_json::{Value, json};

const ECHO: &str = "private echoed provider credential";
const MESSAGE: &str = "The model rejected the current input";

#[tokio::test]
async fn retains_status_for_nonstandard_error_bodies() {
    for remote in [false, true] {
        for status in [401, 403, 429] {
            let raw = format!(
                "HTTP/1.1 {status} Fixture error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{\"message\":\"{ECHO}; authorization {}\"}}",
                server::KEY
            );
            let server =
                Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
            let mut fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            super::retry::http(&fixture);
            let mut provider = fixture.provider.clone();
            provider.request_max_retries = Some(0);
            provider.stream_max_retries = Some(0);
            let Output::Provider(provider) = execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                },
            )
            .await
            else {
                panic!("provider expected")
            };
            fixture.provider = provider;
            let turn = fixture.submit("Answer the task").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            let error = run.error.as_ref().unwrap();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(!error.message.contains("Agent execution failed"));
            assert!(
                error.message.contains(&format!("HTTP {status}")),
                "{error:?}"
            );
            assert_eq!(
                error.details,
                Some(if status == 429 {
                    sailry_protocol::FaultDetails::ModelRecovery {
                        reason: sailry_protocol::conversation::recovery::Stop::AttemptsExhausted,
                    }
                } else {
                    sailry_protocol::FaultDetails::ModelRequest {
                        reason: sailry_protocol::ModelFailure::Unauthorized,
                    }
                })
            );
            let serialized = serde_json::to_string(&page).unwrap();
            assert!(serialized.contains(ECHO));
            assert!(serialized.contains("[redacted]"));
            assert!(!serialized.contains(server::KEY));
            assert!(super::retry::attempts(&page).is_empty());
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 1, "status={status}, remote={remote}");
            }
            assert!(server.upgrades.lock().unwrap().is_empty());
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

fn failure(code: &str, kind: &str) -> Value {
    let detail = json!({"type":"invalid_request_error", "code":code,
        "message":format!("{MESSAGE}; authorization {}", server::KEY),
        "param":format!("Bearer {}", server::KEY)});
    match kind {
        "nested" => json!({"type":"error", "sequence_number":2, "error":detail}),
        "flat" => json!({"type":"error", "sequence_number":2,
            "code":code, "message":detail["message"], "param":detail["param"]}),
        "failed" => json!({"type":"response.failed", "sequence_number":3,
            "response":{"id":"failed", "object":"response", "created_at":1,
                "model":"fixture-a", "status":"failed", "output":[], "error":detail}}),
        _ => panic!("unknown error fixture"),
    }
}

#[tokio::test]
async fn bounds_shared_usage_failures() {
    for remote in [false, true] {
        for code in [
            "subscription_sharing_usage_limit_exceeded",
            "subscription_sharing_usage_unavailable",
        ] {
            for kind in ["nested", "flat", "failed"] {
                let created = json!({"type":"response.created", "sequence_number":1,
                    "response":{"id":"created", "object":"response", "created_at":1,
                        "model":"fixture-a", "status":"in_progress", "output":[]}});
                let server =
                    completion::server(vec![created, failure(code, kind), failure(code, "failed")])
                        .await;
                let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
                super::retry::http(&fixture);
                let turn = fixture.submit("Answer this task").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                let run = page.runs.last().unwrap();
                assert_eq!(run.status, Status::Failed);
                let serialized = serde_json::to_string(&page).unwrap();
                assert_eq!(
                    run.error.as_ref().unwrap().message,
                    format!("{MESSAGE}; authorization [redacted]")
                );
                assert_eq!(
                    run.error.as_ref().unwrap().details,
                    Some(sailry_protocol::FaultDetails::ModelRequest {
                        reason: if code == "subscription_sharing_usage_limit_exceeded" {
                            sailry_protocol::ModelFailure::SharedUsageLimit
                        } else {
                            sailry_protocol::ModelFailure::SharedUsageUnavailable
                        }
                    })
                );
                assert!(!serialized.contains(server::KEY));
                assert_eq!(super::retry::attempts(&page), [1, 2, 3, 4, 5]);
                for part in page.entries.iter().flat_map(|entry| &entry.parts) {
                    if let Part::Resource(value) = part
                        && let Some(retry) =
                            sailry_protocol::conversation::recovery::Retry::from_resource(value)
                    {
                        assert_eq!(retry.code.as_deref(), Some(code));
                        assert_eq!(
                            retry.error.as_deref(),
                            Some(format!("{MESSAGE}; authorization [redacted]").as_str())
                        );
                    }
                }
                assert_eq!(server.requests.lock().unwrap().len(), 6);
                assert!(
                    page.entries
                        .iter()
                        .flat_map(|entry| &entry.parts)
                        .all(|part| {
                            !matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. })
                        })
                );
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn retains_native_stream_errors() {
    for remote in [false, true] {
        for kind in ["nested", "flat", "failed"] {
            let mut events = vec![failure("invalid_prompt", kind)];
            if kind != "failed" {
                events.push(failure("invalid_prompt", "failed"));
            }
            let server = completion::server(events).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            super::retry::http(&fixture);
            let turn = fixture.submit("Answer this task").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            assert_eq!(
                run.error.as_ref().unwrap().message,
                format!("{MESSAGE}; authorization [redacted]")
            );
            assert!(matches!(
                run.error.as_ref().unwrap().details,
                Some(sailry_protocol::FaultDetails::ModelRecovery {
                    reason: sailry_protocol::conversation::recovery::Stop::InvalidPrompt
                })
            ));
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_native_http_errors() {
    for remote in [false, true] {
        let body = json!({"error":{"message":format!("{MESSAGE}; {}", server::KEY),
            "code":"fixture_limit", "type":"invalid_request_error", "param":"input"}});
        let raw = format!(
            "HTTP/1.1 400 Fixture error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
        );
        let server =
            Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        super::retry::http(&fixture);
        let turn = fixture.submit("Answer this task").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(
            run.error.as_ref().unwrap().message,
            format!("{MESSAGE}; [redacted]")
        );
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
