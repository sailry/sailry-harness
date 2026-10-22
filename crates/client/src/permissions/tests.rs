use super::*;
use sailry_protocol::permissions::Kind;
use serde_json::json;

fn package(name: &str, subject: &str) -> plugin::Info {
    serde_json::from_value(json!({
        "summary":{"name":name,"revision":1,"digest":name,"settings_revision":0,"enabled":true},
        "extension":{"api_version":"v1","scope":"host","actions":["computer.read","computer.control"],
            "host":{"entry":"host.js","resources":["host.js"],"handlers":["check","request"],"read_only":["check"]},
            "permissions":(["accessibility","screen_recording"].map(|kind|json!({"kind":kind,"subject":subject,
                "check":"check","request":"request","request_group":"onboarding","actions":["computer.read","computer.control"]})))}
    })).unwrap()
}

#[test]
fn deduplicates_subjects_and_keeps_devices() {
    let device = NodeId([1; 32]);
    let packages = [
        package("first", "helper"),
        package("second", "helper"),
        package("other", "another-helper"),
    ];
    let entries = directory(device, &packages);
    assert_eq!(entries.len(), 4);
    assert!(entries.iter().all(|entry| entry.identity.device == device));
    let next = NodeId([2; 32]);
    assert!(
        directory(next, &packages)
            .iter()
            .all(|entry| entry.identity.device == next)
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.identity.kind == Kind::Accessibility)
            .count(),
        2
    );
}

#[test]
fn aggregates_every_subject() {
    assert_eq!(aggregate([true, true]), Aggregate::Authorized);
    assert_eq!(aggregate([true, false]), Aggregate::Partial);
    assert_eq!(aggregate([false, false]), Aggregate::Unauthorized);
    assert_eq!(aggregate([]), Aggregate::Unauthorized);
}

#[test]
fn lifecycle_removes_only_current_contributions() {
    let mut packages = [
        package("first", "helper"),
        package("other", "another-helper"),
    ];
    let device = NodeId([1; 32]);
    packages[0].summary.enabled = false;
    let enabled = directory(device, &packages);
    assert_eq!(enabled.len(), 2);
    assert!(
        enabled
            .iter()
            .all(|entry| entry.identity.subject == "another-helper")
    );
    assert_eq!(directory(device, &packages[1..]), enabled);
    packages[0].summary.enabled = true;
    assert_eq!(directory(device, &packages).len(), 4);
}

#[test]
fn groups_only_supplied_missing_subjects() {
    let packages = [
        package("first", "helper"),
        package("other", "another-helper"),
    ];
    let entries = directory(NodeId([1; 32]), &packages);
    assert_eq!(requests(&entries).len(), 2);
    assert_eq!(requests(&entries[..2]).len(), 1);
    assert_eq!(requests(&entries[1..2]).len(), 1);
    assert!(requests(std::iter::empty()).is_empty());
    // Deduplication is per invocation; a failed or cancelled request can be retried.
    assert_eq!(requests(&entries[..2]).len(), 1);
}

#[test]
fn rejects_undeclared_handlers_and_capabilities() {
    let mut info = package("first", "helper");
    let extension = info.extension.as_mut().unwrap();
    extension.permissions[0].check = Some("missing".into());
    assert!(!sailry_protocol::permissions::valid(extension));
    extension.permissions[0].check = Some("check".into());
    extension.actions.clear();
    assert!(!sailry_protocol::permissions::valid(extension));
    assert!(directory(NodeId([1; 32]), [&info]).is_empty());
}
