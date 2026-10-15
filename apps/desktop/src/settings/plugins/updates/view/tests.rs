use super::*;

fn row(current: Option<&str>, candidate: Option<&str>) -> Row {
    let info: Info = serde_json::from_value(serde_json::json!({
        "summary": {
            "name": "example", "revision": 1, "settings_revision": 0,
            "digest": "candidate", "version": candidate,
        },
    }))
    .unwrap();
    let mut previous = info.summary.clone();
    previous.version = current.map(str::to_owned);
    Row {
        previous,
        changes: Changes::between(&info, &info),
        info,
        selected: true,
        expanded: false,
        status: Status::Ready,
    }
}

#[core::prelude::v1::test]
fn preserves_equal_versions() {
    assert_eq!(row(Some("0.1.0"), Some("0.1.0")).version(), "0.1.0 → 0.1.0");
}

#[core::prelude::v1::test]
fn preserves_current_and_candidate_order() {
    assert_eq!(row(Some("0.1.4"), Some("0.1.0")).version(), "0.1.4 → 0.1.0");
}

#[core::prelude::v1::test]
fn leaves_missing_versions_unknown() {
    assert_eq!(row(None, None).version(), "— → —");
    assert_eq!(row(None, Some("0.1.0")).version(), "— → 0.1.0");
    assert_eq!(row(Some("0.1.0"), None).version(), "0.1.0 → —");
}
