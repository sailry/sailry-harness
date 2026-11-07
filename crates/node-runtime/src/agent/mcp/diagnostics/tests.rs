use super::*;
use serde_json::json;

#[derive(Clone, Default)]
struct Levels(std::sync::Arc<std::sync::Mutex<Vec<tracing::Level>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Levels {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        self.0.lock().unwrap().push(*event.metadata().level());
    }
}

#[test]
fn warns_for_server_tool_failures() {
    use tracing_subscriber::layer::SubscriberExt;

    let levels = Levels::default();
    let subscriber = tracing_subscriber::registry().with(levels.clone());
    tracing::subscriber::with_default(subscriber, || {
        let mut span = call();
        external(&mut span);
        finish(&mut span, &Ok(CallToolResult::error(Vec::new())), false);
    });
    let levels = levels.0.lock().unwrap();
    assert_eq!(levels.last(), Some(&tracing::Level::WARN));
    assert!(!levels.contains(&tracing::Level::ERROR));
}

#[test]
fn keeps_transport_failures_visible() {
    use tracing_subscriber::layer::SubscriberExt;

    let levels = Levels::default();
    let subscriber = tracing_subscriber::registry().with(levels.clone());
    tracing::subscriber::with_default(subscriber, || {
        let mut span = call();
        external(&mut span);
        finish(
            &mut span,
            &Err(Fault::new(ErrorCode::Unavailable, "transport failed")),
            false,
        );
    });
    assert_eq!(
        levels.0.lock().unwrap().last(),
        Some(&tracing::Level::ERROR)
    );
}

fn call() -> Span {
    begin(
        Context {
            call: Some(sailry_protocol::RequestId::new()),
            handler: Some("same_tool".into()),
            ..Default::default()
        },
        "call",
        &CancellationToken::new(),
    )
}

#[test]
fn separates_server_errors_from_business_results() {
    let mut failed = call();
    external(&mut failed);
    finish(
        &mut failed,
        &Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text("PRIVATE_SERVER_ERROR"),
        ])),
        false,
    );
    assert!(matches!(failed.entry.phase, Phase::Failed));
    assert_eq!(failed.entry.reason, Some("server_error"));
    assert_eq!(failed.entry.communication_outcome, Some("completed"));
    assert_eq!(failed.entry.execution_outcome, Some("failed"));
    for value in [
        json!({"output":{"error":"PRIVATE_BUSINESS_FIELD"}}),
        json!({"output":{"isError":true,"error":"domain row"}}),
        json!({"output":{"error":{"code":"outcome_unknown"}}}),
        json!({"output":"PRIVATE_ERROR_TEXT","error":"business"}),
        json!({"error":null}),
        json!({"error":{"code":"business"}}),
    ] {
        let mut success = call();
        external(&mut success);
        finish(&mut success, &Ok(CallToolResult::structured(value)), false);
        assert!(matches!(success.entry.phase, Phase::Completed));
        assert_eq!(success.entry.communication_outcome, Some("completed"));
        assert_eq!(success.entry.execution_outcome, Some("succeeded"));
    }
}

#[test]
fn preserves_unknown_and_cancelled_outcomes() {
    let mut unknown = call();
    external(&mut unknown);
    finish(
        &mut unknown,
        &Err(Fault::new(ErrorCode::OutcomeUnknown, "PRIVATE_MESSAGE")),
        false,
    );
    assert!(matches!(unknown.entry.phase, Phase::Interrupted));
    assert_eq!(unknown.entry.code, Some(ErrorCode::OutcomeUnknown));
    assert_eq!(unknown.entry.communication_outcome, Some("unknown"));
    assert_eq!(unknown.entry.execution_outcome, Some("unknown"));
    for dispatched in [false, true] {
        let mut cancelled = call();
        if dispatched {
            external(&mut cancelled);
        }
        finish(
            &mut cancelled,
            &Err(Fault::new(ErrorCode::Cancelled, "PRIVATE_CANCEL_MESSAGE")),
            true,
        );
        assert!(matches!(cancelled.entry.phase, Phase::Cancelled));
        assert_eq!(
            cancelled.entry.communication_outcome,
            Some(if dispatched {
                "cancelled"
            } else {
                "not_started"
            })
        );
        assert_eq!(
            cancelled.entry.execution_outcome,
            Some(if dispatched { "unknown" } else { "not_started" })
        );
    }
}

#[tokio::test]
async fn correlates_concurrent_same_name_calls() {
    let barrier = tokio::sync::Barrier::new(2);
    let invoke = || async {
        let mut call = call();
        let identity = call.entry.context.clone();
        let begin = identity.call.unwrap();
        let stop = CancellationToken::new();
        stage(
            &mut call,
            &stop,
            "authorization",
            ErrorCode::PermissionDenied,
            async {
                barrier.wait().await;
                Ok::<_, ()>(())
            },
        )
        .await
        .unwrap();
        let mut roundtrip = begin_external(&identity);
        let value = Ok(CallToolResult::structured(
            json!({"output":"PRIVATE_RESULT"}),
        ));
        finish(&mut roundtrip, &value, false);
        finish(&mut call, &value, false);
        assert_eq!(call.entry.context.call, Some(begin));
        assert_eq!(roundtrip.entry.context.call, Some(begin));
        assert_eq!(call.entry.context.handler, roundtrip.entry.context.handler);
        begin
    };
    let (first, second) = futures::join!(invoke(), invoke());
    assert_ne!(first, second);
}

#[tokio::test]
async fn keeps_authorization_failure_without_dispatch() {
    let mut call = call();
    let stop = CancellationToken::new();
    let error = stage(
        &mut call,
        &stop,
        "authorization",
        ErrorCode::PermissionDenied,
        async {
            Err::<(), _>(Fault::new(
                ErrorCode::PermissionDenied,
                "PRIVATE_AUTHORIZATION_ERROR",
            ))
        },
    )
    .await;
    finish(
        &mut call,
        &error.map(|()| CallToolResult::success(Vec::new())),
        false,
    );
    assert!(matches!(call.entry.phase, Phase::Failed));
    assert_eq!(call.entry.code, Some(ErrorCode::PermissionDenied));
    assert_eq!(call.entry.communication_outcome, Some("not_started"));
    assert_eq!(call.entry.execution_outcome, Some("not_started"));
}

fn begin_external(identity: &Context) -> Span {
    let mut span = begin(
        identity.clone(),
        "external_roundtrip",
        &CancellationToken::new(),
    );
    external(&mut span);
    span
}
