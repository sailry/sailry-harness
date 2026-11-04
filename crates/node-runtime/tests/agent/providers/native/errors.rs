use super::*;
use serde_json::json;

const ECHO: &str = "private echoed provider credential";
const MESSAGE: &str = "The model rejected the current input";

#[tokio::test]
async fn preserves_failure_when_observation_write_fails() {
    for remote in [false, true] {
        for status in [401, 429] {
            let raw = format!(
                "HTTP/1.1 {status} Fixture error\r\nContent-Type: application/json\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{{\"message\":\"{status}: {MESSAGE}\"}}"
            );
            let server =
                Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
            let mut baseline = None;
            for reject_observation in [false, true] {
                let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
                let before = server.requests.lock().unwrap().len();
                if reject_observation {
                    fixture.node.with_storage(|db| Box::pin(async move {
                db.execute_batch("CREATE TRIGGER reject_observation BEFORE INSERT ON agent_events WHEN json_type(NEW.body,'$.metadata.execution.observation')='object' BEGIN SELECT RAISE(ABORT,'injected observation failure'); END;")
                    .await.map_err(crate::database_fixture::fault)
            })).await.unwrap();
                }
                let turn = fixture.submit("Keep the provider rejection").await;
                let page = finished_within(
                    &fixture.client,
                    fixture.session.id,
                    turn,
                    retry_fixture::budget(),
                )
                .await;
                let run = page.runs.last().unwrap();
                assert_eq!(run.status, Status::Failed);
                let error = run.error.as_ref().unwrap();
                if status == 401 {
                    assert_eq!(
                        error.details,
                        Some(sailry_protocol::FaultDetails::ModelRequest {
                            reason: sailry_protocol::ModelFailure::Unauthorized,
                        })
                    );
                }
                assert!(error.message.contains(MESSAGE), "{error:?}");
                assert!(error.message.contains(&status.to_string()), "{error:?}");
                assert!(!error.message.contains("observation"), "{error:?}");
                if let Some(expected) = &baseline {
                    assert_eq!(error, expected);
                } else {
                    baseline = Some(error.clone());
                }
                assert_eq!(
                    server.requests.lock().unwrap().len() - before,
                    if status == 401 { 1 } else { 6 }
                );
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn retains_rejected_requests() {
    for remote in [false, true] {
        let mut fixture = Fixture::empty(remote, ModelApi::Responses, "http://127.0.0.1:9").await;
        for (case, status) in [
            ("unauthorized", 401),
            ("forbidden", 403),
            ("http", 400),
            ("stream", 200),
        ] {
            let server = if status == 200 {
                let event = json!({"type":"error","error":{"message":format!("{MESSAGE}; authorization {}",server::KEY)}});
                completion::server(vec![event]).await
            } else {
                let body = if status == 400 {
                    json!({"error":{"message":format!("{MESSAGE}; {}",server::KEY),"code":"fixture_limit","type":"invalid_request_error","param":"input"}})
                } else {
                    json!({"message":format!("{ECHO}; authorization {}",server::KEY)})
                };
                let raw = format!(
                    "HTTP/1.1 {status} Fixture error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
                );
                Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await
            };
            fixture
                .select(case, ModelApi::Responses, &server.endpoint)
                .await;
            let turn = fixture.submit("Answer this task").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed, "{case}, remote={remote}");
            let error = run.error.as_ref().unwrap();
            let serialized = serde_json::to_string(&page).unwrap();
            if status == 401 || status == 403 {
                assert_eq!(
                    error.code,
                    ErrorCode::Unavailable,
                    "{case}, remote={remote}"
                );
                assert!(
                    error.message.contains(&status.to_string()),
                    "{case}, remote={remote}: {error:?}"
                );
                assert_eq!(
                    error.details,
                    Some(sailry_protocol::FaultDetails::ModelRequest {
                        reason: sailry_protocol::ModelFailure::Unauthorized,
                    }),
                    "{case}, remote={remote}"
                );
                assert!(serialized.contains(ECHO), "{case}, remote={remote}");
                assert!(serialized.contains("[redacted]"), "{case}, remote={remote}");
                crate::observations::assert_context_hidden(&fixture.node, fixture.session.id).await;
                assert!(
                    crate::agent_support::turn_context(&server.requests.lock().unwrap()[0].body)
                        .is_some(),
                    "{case}, remote={remote}"
                );
            } else {
                assert!(
                    error.message.contains(MESSAGE),
                    "{case}, remote={remote}: {error:?}"
                );
                assert!(
                    error.message.contains("[redacted]"),
                    "{case}, remote={remote}: {error:?}"
                );
                if status == 200 {
                    assert_eq!(
                        error.details,
                        Some(sailry_protocol::FaultDetails::ModelRequest {
                            reason: sailry_protocol::ModelFailure::Request,
                        }),
                        "{case}, remote={remote}"
                    );
                }
            }
            assert!(!serialized.contains(server::KEY), "{case}, remote={remote}");
            assert_eq!(
                server.requests.lock().unwrap().len(),
                1,
                "{case}, remote={remote}"
            );
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
