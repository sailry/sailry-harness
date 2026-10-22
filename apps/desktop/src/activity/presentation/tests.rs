use super::*;
use sailry_client::activity::Id;
use sailry_protocol::{
    ErrorCode, Fault, NodeId, NotificationId, SessionId, TurnId, notification::Diagnostic,
};

#[test]
fn reports_exclude_user_content_and_raw_errors() {
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
            }),
            version: "0.1.0".into(),
            platform: "macos-aarch64".into(),
        })),
    };
    let report = report(&notice).unwrap();
    assert!(report.contains("canonical-event"));
    assert!(report.contains("request_timeout"));
    for text in [
        &notice.title,
        &notice.message,
        &notice.error.as_ref().unwrap().message,
    ] {
        assert!(!report.contains(text));
    }
    notice.details = Some(Details::Text {
        body: "Private plugin content".into(),
    });
    assert!(super::report(&notice).is_none());
}
