use super::*;

#[test]
fn preserves_bounded_diagnostic_causes() {
    let mut report = SystemDiagnostic::new(
        crate::RequestId::new(),
        SystemSource::GitInspection,
        crate::ErrorCode::Busy,
    );
    let wire = serde_json::to_value(&report).unwrap();
    assert!(!wire.as_object().unwrap().contains_key("cause"));
    assert_eq!(
        serde_json::from_value::<SystemDiagnostic>(wire)
            .unwrap()
            .cause,
        None
    );
    for cause in [
        DiagnosticCause::GitQueueTimeout,
        DiagnosticCause::GitReadTimeout,
        DiagnosticCause::GitLocked,
    ] {
        report.cause = Some(cause);
        let mut wire = serde_json::to_value(&report).unwrap();
        assert_eq!(wire["cause"], cause.as_str());
        assert_eq!(
            serde_json::from_value::<SystemDiagnostic>(wire.clone()).unwrap(),
            report
        );
        wire["cause"] = serde_json::json!("PRIVATE_ERROR_MESSAGE");
        assert!(serde_json::from_value::<SystemDiagnostic>(wire).is_err());
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "omitted_details_remain_empty",
            (|scenario: &'static str| {
                let draft: Draft = serde_json::from_value(serde_json::json!({
                    "title": "Reminder", "message": "Review", "kind": "info", "session": null
                }))
                .unwrap();
                assert!(draft.details.is_none(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "details_keep_their_type",
            (|scenario: &'static str| {
                let details = Details::Text {
                    body: "A completed workflow result".into(),
                };
                let wire = serde_json::to_value(&details).unwrap();
                assert_eq!(wire["type"], "text", "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Details>(wire).unwrap(),
                    details,
                    "{scenario}"
                );
            }),
        ),
        (
            "system_reports_reject_payload_fields",
            (|scenario: &'static str| {
                let report = SystemDiagnostic::new(
                    crate::RequestId::new(),
                    SystemSource::PluginCallback,
                    crate::ErrorCode::Expired,
                );
                assert!(report.valid(), "{scenario}");
                let mut wire = serde_json::to_value(&report).unwrap();
                wire["body"] = serde_json::json!("PRIVATE_BODY");
                assert!(
                    serde_json::from_value::<SystemDiagnostic>(wire).is_err(),
                    "{scenario}"
                );
                let mut invalid = report.clone();
                invalid.operation = Some("fetch_web secret=PRIVATE".into());
                assert!(!invalid.valid(), "{scenario}");
                let mut expected = serde_json::to_value(&report).unwrap();
                expected["kind"] = serde_json::json!("system_diagnostic");
                let details = crate::FaultDetails::SystemDiagnostic(Box::new(report));
                let encoded = serde_json::to_value(&details).unwrap();
                assert_eq!(encoded, expected, "{scenario}");
                assert_eq!(
                    serde_json::from_value::<crate::FaultDetails>(encoded).unwrap(),
                    details,
                    "{scenario}"
                );
                let fault = crate::Fault {
                    code: crate::ErrorCode::Expired,
                    message: "Callback deadline elapsed".into(),
                    details: Some(details),
                };
                let wire = serde_json::to_value(&fault).unwrap();
                assert_eq!(
                    wire,
                    serde_json::json!({
                        "code": "expired",
                        "message": "Callback deadline elapsed",
                        "details": expected,
                    }),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<crate::Fault>(wire).unwrap(),
                    fault,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
