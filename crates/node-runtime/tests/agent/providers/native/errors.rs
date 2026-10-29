use super::*;
use serde_json::json;

const ECHO: &str = "private echoed provider credential";
const MESSAGE: &str = "The model rejected the current input";

#[tokio::test]
async fn preserves_failure_when_observation_write_fails() {
    for remote in [false, true] {
        for status in [401, 429] {
            let raw = format!(
                "HTTP/1.1 {status} Fixture error\r\nContent-Type: application/json\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{{\"message\":\"{MESSAGE}\"}}"
            );
            let server =
                Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            fixture.node.with_storage(|db| Box::pin(async move {
                db.execute_batch("CREATE TRIGGER reject_observation BEFORE INSERT ON agent_events WHEN json_type(NEW.body,'$.metadata.execution.observation')='object' BEGIN SELECT RAISE(ABORT,'injected observation failure'); END;")
                    .await.map_err(crate::database_fixture::fault)
            })).await.unwrap();
            let turn = fixture.submit("Keep the provider rejection").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed);
            let error = run.error.as_ref().unwrap();
            assert_eq!(
                error.details,
                Some(sailry_protocol::FaultDetails::ModelRequest {
                    reason: if status == 401 {
                        sailry_protocol::ModelFailure::Unauthorized
                    } else {
                        sailry_protocol::ModelFailure::RateLimited
                    },
                })
            );
            assert!(!error.message.contains("observation"), "{error:?}");
            assert_eq!(
                server.requests.lock().unwrap().len(),
                if status == 401 {
                    1
                } else {
                    1 + goose_provider_types::retry::DEFAULT_MAX_RETRIES
                }
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_authentication_failures() {
    for remote in [false, true] {
        for status in [401, 403] {
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
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(error.message.contains(&status.to_string()), "{error:?}");
            assert_eq!(
                error.details,
                Some(sailry_protocol::FaultDetails::ModelRequest {
                    reason: sailry_protocol::ModelFailure::Unauthorized,
                })
            );
            let serialized = serde_json::to_string(&page).unwrap();
            assert!(serialized.contains(ECHO));
            assert!(serialized.contains("[redacted]"));
            assert!(!serialized.contains(server::KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_stream_errors() {
    for remote in [false, true] {
        let event = json!({"type":"error","error":{"message":format!("{MESSAGE}; authorization {}",server::KEY)}});
        let server = completion::server(vec![event]).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Answer this task").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        let error = run.error.as_ref().unwrap();
        assert!(error.message.contains(MESSAGE));
        assert!(error.message.contains("[redacted]"));
        assert_eq!(
            error.details,
            Some(sailry_protocol::FaultDetails::ModelRequest {
                reason: sailry_protocol::ModelFailure::Request,
            })
        );
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_http_errors() {
    for remote in [false, true] {
        let body = json!({"error":{"message":format!("{MESSAGE}; {}",server::KEY),"code":"fixture_limit","type":"invalid_request_error","param":"input"}});
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
        let error = run.error.as_ref().unwrap();
        assert!(error.message.contains(MESSAGE));
        assert!(error.message.contains("[redacted]"));
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1 + goose_provider_types::retry::DEFAULT_MAX_RETRIES
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
