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
fn aggregates_scoped_subjects_through_lifecycle() {
    for (case_name, run) in [
        (
            "deduplicates_subjects_and_keeps_devices",
            (|case_name| {
                let device = NodeId([1; 32]);
                let packages = [
                    package("first", "helper"),
                    package("second", "helper"),
                    package("other", "another-helper"),
                ];
                let entries = directory(device, &packages);
                assert_eq!(entries.len(), 4, "{case_name}");
                assert!(
                    entries.iter().all(|entry| entry.identity.device == device),
                    "{case_name}"
                );
                let next = NodeId([2; 32]);
                assert!(
                    directory(next, &packages)
                        .iter()
                        .all(|entry| entry.identity.device == next),
                    "{case_name}"
                );
                assert_eq!(
                    entries
                        .iter()
                        .filter(|entry| entry.identity.kind == Kind::Accessibility)
                        .count(),
                    2,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "aggregates_every_subject",
            (|case_name| {
                assert_eq!(
                    aggregate([true, true]),
                    Aggregate::Authorized,
                    "{case_name}"
                );
                assert_eq!(aggregate([true, false]), Aggregate::Partial, "{case_name}");
                assert_eq!(
                    aggregate([false, false]),
                    Aggregate::Unauthorized,
                    "{case_name}"
                );
                assert_eq!(aggregate([]), Aggregate::Unauthorized, "{case_name}");
            }) as fn(&str),
        ),
        (
            "lifecycle_removes_only_current_contributions",
            (|case_name| {
                let mut packages = [
                    package("first", "helper"),
                    package("other", "another-helper"),
                ];
                let device = NodeId([1; 32]);
                packages[0].summary.enabled = false;
                let enabled = directory(device, &packages);
                assert_eq!(enabled.len(), 2, "{case_name}");
                assert!(
                    enabled
                        .iter()
                        .all(|entry| entry.identity.subject == "another-helper"),
                    "{case_name}"
                );
                assert_eq!(directory(device, &packages[1..]), enabled, "{case_name}");
                packages[0].summary.enabled = true;
                assert_eq!(directory(device, &packages).len(), 4, "{case_name}");
            }) as fn(&str),
        ),
        (
            "groups_only_supplied_missing_subjects",
            (|case_name| {
                let packages = [
                    package("first", "helper"),
                    package("other", "another-helper"),
                ];
                let entries = directory(NodeId([1; 32]), &packages);
                assert_eq!(requests(&entries).len(), 2, "{case_name}");
                assert_eq!(requests(&entries[..2]).len(), 1, "{case_name}");
                assert_eq!(requests(&entries[1..2]).len(), 1, "{case_name}");
                assert!(requests(std::iter::empty()).is_empty(), "{case_name}");
                // Deduplication is per invocation; a failed or cancelled request can be retried.
                assert_eq!(requests(&entries[..2]).len(), 1, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
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
