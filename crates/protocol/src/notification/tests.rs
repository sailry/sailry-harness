use super::*;

#[test]
fn omitted_details_remain_empty() {
    let draft: Draft = serde_json::from_value(serde_json::json!({
        "title": "Reminder", "message": "Review", "kind": "info", "session": null
    }))
    .unwrap();
    assert!(draft.details.is_none());
}

#[test]
fn details_keep_their_type() {
    let details = Details::Text {
        body: "A completed workflow result".into(),
    };
    let wire = serde_json::to_value(&details).unwrap();
    assert_eq!(wire["type"], "text");
    assert_eq!(serde_json::from_value::<Details>(wire).unwrap(), details);
}

#[test]
fn system_reports_reject_payload_fields() {
    let report = SystemDiagnostic::new(
        crate::RequestId::new(),
        SystemSource::PluginCallback,
        crate::ErrorCode::Expired,
    );
    assert!(report.valid());
    let mut wire = serde_json::to_value(&report).unwrap();
    wire["body"] = serde_json::json!("PRIVATE_BODY");
    assert!(serde_json::from_value::<SystemDiagnostic>(wire).is_err());
    let mut invalid = report.clone();
    invalid.operation = Some("fetch_web secret=PRIVATE".into());
    assert!(!invalid.valid());
    let details = crate::FaultDetails::SystemDiagnostic(report);
    let encoded = serde_json::to_value(&details).unwrap();
    assert_eq!(encoded["kind"], "system_diagnostic");
    assert_eq!(
        serde_json::from_value::<crate::FaultDetails>(encoded).unwrap(),
        details
    );
}
