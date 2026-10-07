use super::*;
use serde_json::{Value, json};

const PRIVATE: &str = "private echoed provider credential";

fn failure(code: &str, kind: &str) -> Value {
    let detail = json!({"type":"invalid_request_error", "code":code,
        "message":PRIVATE, "param":format!("Bearer {}", server::KEY)});
    match kind {
        "nested" => json!({"type":"error", "sequence_number":2, "error":detail}),
        "flat" => json!({"type":"error", "sequence_number":2,
            "code":code, "message":PRIVATE, "param":detail["param"]}),
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
                let fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
                let turn = fixture.submit("Answer this task").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                let run = page.runs.last().unwrap();
                assert_eq!(run.status, Status::Failed);
                assert_eq!(
                    run.error.as_ref().unwrap().message,
                    format!("Agent execution failed (model.openai_responses.{code})")
                );
                let serialized = serde_json::to_string(&page).unwrap();
                assert!(!serialized.contains(PRIVATE));
                assert!(!serialized.contains(server::KEY));
                assert_eq!(server.requests.lock().unwrap().len(), 1);
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn sanitizes_unknown_stream_causes() {
    for remote in [false, true] {
        let server = completion::server(vec![failure(PRIVATE, "nested")]).await;
        let fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Answer this task").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(
            run.error.as_ref().unwrap().message,
            "Agent execution failed (model.openai_responses.response_failed)"
        );
        let serialized = serde_json::to_string(&page).unwrap();
        assert!(!serialized.contains(PRIVATE));
        assert!(!serialized.contains(server::KEY));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
