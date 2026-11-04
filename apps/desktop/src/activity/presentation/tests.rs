use super::*;
use sailry_client::activity::Id;
use sailry_protocol::{
    ErrorCode, Fault, NodeId, NotificationId, SessionId, TurnId,
    conversation::compaction::{Failure, Reason, Stage},
    notification::Diagnostic,
};

#[test]
fn content_and_diagnostics() {
    for (scenario, check) in [
        (
            "reports_exclude_user_content_and_raw_errors",
            (|_scenario: &str| {
                let mut notice = Notice {
                    id: Id {
                        node: NodeId([4; 32]),
                        cursor: 9,
                        target: Target::Notification(NotificationId::new()),
                    },
                    project: None,
                    worktree: None,
                    title: "Private conversation title".into(),
                    message: "Private message content".into(),
                    error: Some(Fault::new(
                        ErrorCode::Unavailable,
                        "Sensitive provider response",
                    )),
                    presentation: None,
                    kind: Kind::Failed,
                    read: false,
                    package: String::new(),
                    session: Some(SessionId::new()),
                    timestamp_ms: Some(1000),
                    details: Some(Details::Diagnostic(Diagnostic {
                        event: "canonical-event".into(),
                        sequence: 7,
                        turn: TurnId::new(),
                        cause: Cause::Compaction(Failure {
                            source: Source::Manual,
                            stage: Stage::Request,
                            reason: Reason::RequestTimeout,
                            continued: false,
                            status: Some(504),
                            cause: None,
                        }),
                        version: "0.1.0".into(),
                        platform: "macos-aarch64".into(),
                    })),
                };
                let report = report(&notice).unwrap();
                assert!(
                    report.contains("canonical-event"),
                    "reports exclude user content and raw errors"
                );
                assert!(
                    report.contains("request_timeout"),
                    "reports exclude user content and raw errors"
                );
                for text in [
                    &notice.title,
                    &notice.message,
                    &notice.error.as_ref().unwrap().message,
                ] {
                    assert!(
                        !report.contains(text),
                        "reports exclude user content and raw errors"
                    );
                }
                let mut diagnostic = sailry_protocol::notification::SystemDiagnostic::new(
                    sailry_protocol::RequestId::new(),
                    sailry_protocol::notification::SystemSource::PluginCallback,
                    sailry_protocol::ErrorCode::OutcomeUnknown,
                );
                diagnostic.reason = sailry_protocol::notification::DiagnosticReason::Timeout;
                diagnostic.operation = Some("write_plugin_value".into());
                notice.details = Some(Details::System(diagnostic.clone()));
                let copied = super::report(&notice).unwrap();
                assert!(
                    copied.contains(&diagnostic.incident.to_string()),
                    "reports exclude user content and raw errors"
                );
                assert!(
                    copied.contains("write_plugin_value"),
                    "reports exclude user content and raw errors"
                );
                assert!(
                    copied.contains("outcome_unknown"),
                    "reports exclude user content and raw errors"
                );
                assert!(
                    !copied.contains("Private"),
                    "reports exclude user content and raw errors"
                );
                assert!(
                    !copied.contains("Sensitive"),
                    "reports exclude user content and raw errors"
                );
                assert_eq!(
                    system_key(&diagnostic),
                    "chat_execution_unknown",
                    "reports exclude user content and raw errors"
                );
                diagnostic.code = ErrorCode::Expired;
                assert_eq!(
                    system_key(&diagnostic),
                    "plugin_response_timeout",
                    "reports exclude user content and raw errors"
                );
                diagnostic.code = ErrorCode::Cancelled;
                assert_eq!(
                    system_key(&diagnostic),
                    "turn_cancelled",
                    "reports exclude user content and raw errors"
                );
                notice.details = Some(Details::Text {
                    body: "Private plugin content".into(),
                });
                assert!(
                    super::report(&notice).is_none(),
                    "reports exclude user content and raw errors"
                );
            }) as fn(&str),
        ),
        (
            "preserves_original_content_and_error_text",
            (|_scenario: &str| {
                let mut notice = notice();
                notice.message = "Original response\n<script>literal text</script>".into();
                notice.error = Some(Fault::new(
                    ErrorCode::Unavailable,
                    "Original error\nHTTP 200 was received",
                ));
                notice.details = Some(Details::Diagnostic(Diagnostic {
                event: "canonical-event".into(),
                sequence: 7,
                turn: TurnId::new(),
                cause: Cause::Compaction(Failure {
                    source: Source::Manual,
                    stage: Stage::Summary,
                    reason: Reason::NoReduction,
                    continued: false,
                    status: Some(200),
                    cause: Some(
                "context summary did not reduce the conversation\nOriginal validation error".into(),
                    ),
                }),
                version: "0.1.0".into(),
                platform: "macos-aarch64".into(),
            }));
                assert_eq!(
                    content(&notice),
                    [
                        "Original response\n<script>literal text</script>",
                        "context summary did not reduce the conversation\nOriginal validation error",
                        "Original error\nHTTP 200 was received",
                    ],
                    "preserves original content and error text"
                );
                notice.message.clear();
                notice.error = None;
                if let Some(Details::Diagnostic(diagnostic)) = &mut notice.details
                    && let Cause::Compaction(failure) = &mut diagnostic.cause
                {
                    failure.cause = None;
                }
                assert!(
                    content(&notice).is_empty(),
                    "preserves original content and error text"
                );
                let report = report(&notice).unwrap();
                assert!(
                    report.contains("no_reduction"),
                    "preserves original content and error text"
                );
                assert!(
                    report.contains("200"),
                    "preserves original content and error text"
                );
            }),
        ),
        (
            "omits_only_exact_duplicate_text",
            (|_scenario: &str| {
                let mut notice = notice();
                notice.message = "  Original line\nSecond line  ".into();
                notice.details = Some(Details::Text {
                    body: notice.message.clone(),
                });
                notice.error = Some(Fault::new(ErrorCode::Unavailable, notice.message.clone()));
                assert_eq!(
                    content(&notice),
                    ["  Original line\nSecond line  "],
                    "omits only exact duplicate text"
                );
                notice.message.clear();
                notice.details = Some(Details::Task(sailry_protocol::notification::Task {
                    turn: TurnId::new(),
                    status: Status::Failed,
                    started_ms: None,
                    finished_ms: None,
                    error: Some(ErrorCode::Unavailable),
                }));
                notice.error = Some(Fault::new(ErrorCode::Unavailable, ""));
                assert!(
                    content(&notice).is_empty(),
                    "omits only exact duplicate text"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

fn notice() -> Notice {
    Notice {
        id: Id {
            node: NodeId([4; 32]),
            cursor: 9,
            target: Target::Notification(NotificationId::new()),
        },
        project: None,
        worktree: None,
        title: "Original title".into(),
        message: String::new(),
        error: None,
        presentation: None,
        kind: Kind::Failed,
        read: false,
        package: String::new(),
        session: Some(SessionId::new()),
        timestamp_ms: Some(1000),
        details: None,
    }
}
