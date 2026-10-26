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
        blocked: None,
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

fn entry(candidate: bool) -> Entry {
    let previous = row(Some("0.1.0"), Some("0.1.0")).info;
    let mut info = previous.clone();
    info.summary.version = Some("0.1.1".into());
    Entry {
        current: previous.summary.clone(),
        candidate: candidate.then_some(Candidate::Node(info)),
        previous: Some(previous),
        manual: false,
        blocked: Some(Blocked {
            version: "0.2.0".into(),
            required: "0.1.0-alpha.5".into(),
            release_notes: None,
        }),
    }
}

#[core::prelude::v1::test]
fn excludes_blocked_updates_from_selection() {
    let row = Row::new(&entry(false)).unwrap();
    assert!(!row.selected);
    assert!(row.status == Status::Blocked);
    assert_eq!(row.version(), "0.1.0 → 0.2.0");
}

#[core::prelude::v1::test]
fn preserves_compatible_candidate_with_blocked_release() {
    let row = Row::new(&entry(true)).unwrap();
    assert!(row.selected);
    assert!(row.status == Status::Ready);
    assert_eq!(row.version(), "0.1.0 → 0.1.1");
    assert_eq!(row.blocked.unwrap().version, "0.2.0");
}
