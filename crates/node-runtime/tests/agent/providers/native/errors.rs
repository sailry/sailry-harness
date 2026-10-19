use super::*;
use serde_json::{Value, json};

const ECHO: &str = "private echoed provider credential";
const MESSAGE: &str = "The model rejected the current input";

#[tokio::test]
async fn retains_status_for_nonstandard_error_bodies() {
    for remote in [false, true] {
        for (status, category, attempts) in [
            (401, "unauthorized", 1),
            (403, "forbidden", 1),
            (429, "rate_limited", 6),
        ] {
            let raw = format!(
                "HTTP/1.1 {status} Fixture error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{\"message\":\"{ECHO}; authorization {}\"}}",
                server::KEY
            );
            let server =
                Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Answer the task").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            let error = run.error.as_ref().unwrap();
            assert!(
                error
                    .message
                    .contains(&format!("model.openai_responses.{category}")),
                "{error:?}"
            );
            assert!(
                error.message.contains(&format!("HTTP {status}")),
                "{error:?}"
            );
            let serialized = serde_json::to_string(&page).unwrap();
            assert!(serialized.contains(ECHO));
            assert!(serialized.contains("[redacted]"));
            assert!(!serialized.contains(server::KEY));
            // Node retries a rate-limited opening five times before any output.
            // The Responses transport and client have no additional retry layer.
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), attempts, "status={status}, remote={remote}");
                assert!(
                    requests
                        .iter()
                        .all(|request| request.body == requests[0].body)
                );
            }
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
async fn retains_shared_usage_causes_without_replay() {
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
                let turn = fixture.submit("Answer this task").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                let run = page.runs.last().unwrap();
                assert_eq!(run.status, Status::Failed);
                assert_eq!(
                    run.error.as_ref().unwrap().message.lines().next().unwrap(),
                    format!("Agent execution failed (model.openai_responses.{code})")
                );
                let serialized = serde_json::to_string(&page).unwrap();
                assert!(serialized.contains(MESSAGE));
                assert!(serialized.contains("[redacted]"));
                assert!(!serialized.contains(server::KEY));
                assert_eq!(server.requests.lock().unwrap().len(), 1);
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn retains_upstream_stream_causes() {
    for remote in [false, true] {
        for kind in ["nested", "flat", "failed"] {
            let server = completion::server(vec![failure("fixture_limit", kind)]).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Answer this task").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            let (summary, body) = run
                .error
                .as_ref()
                .unwrap()
                .message
                .split_once("\n\n")
                .unwrap();
            assert_eq!(
                summary,
                "Agent execution failed (model.openai_responses.response_failed)"
            );
            let detail: Value = serde_json::from_str(body).unwrap();
            assert_eq!(detail["code"], "fixture_limit");
            assert_eq!(
                detail["message"],
                format!("{MESSAGE}; authorization [redacted]")
            );
            assert_eq!(detail["param"], "Bearer [redacted]");
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_upstream_http_causes() {
    for remote in [false, true] {
        let body = json!({"error":{"message":format!("{MESSAGE}; {}", server::KEY),
            "code":"fixture_limit", "type":"invalid_request_error", "param":"input"}});
        let raw = format!(
            "HTTP/1.1 400 Fixture error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
        );
        let server =
            Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Answer this task").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        let (summary, body) = run
            .error
            .as_ref()
            .unwrap()
            .message
            .split_once("\n\n")
            .unwrap();
        assert_eq!(
            summary,
            "Agent execution failed (model.openai_responses.invalid_request); HTTP 400; invalid_input"
        );
        let detail: Value = serde_json::from_str(body).unwrap();
        assert_eq!(detail["message"], format!("{MESSAGE}; [redacted]"));
        assert_eq!(detail["code"], "fixture_limit");
        assert_eq!(detail["type"], "invalid_request_error");
        assert_eq!(detail["param"], "input");
        assert_eq!(detail["status"], 400);
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
