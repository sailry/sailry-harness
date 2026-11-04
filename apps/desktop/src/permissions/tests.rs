use super::*;
use core::prelude::v1::test;

fn action() -> Action {
    Rc::new(|_, _| unreachable!("static action selection only"))
}

fn card(status: Status, settings: bool, request: bool) -> Card {
    Card {
        owner: None,
        resource: Resource::FullDisk,
        status,
        settings: settings.then_some(app::DISK_SETTINGS),
        check: Some(action()),
        request: request.then(action),
        requires: None,
    }
}

#[test]
fn member_actions() {
    for (scenario, check) in [
        (
            "merged_disk_members_keep_native_settings",
            (|_scenario: &str| {
                let native = card(Status::Required, true, false);
                let plugin = card(Status::Required, false, true);
                for members in [
                    vec![native.clone(), plugin.clone()],
                    vec![plugin.clone(), native.clone()],
                ] {
                    let rows = catalog::rows(&members);
                    assert_eq!(rows.len(), 1, "merged disk members keep native settings");
                    assert_eq!(
                        rows[0].status,
                        Status::Required,
                        "merged disk members keep native settings"
                    );
                    assert_eq!(
                        rows[0].settings,
                        Some(app::DISK_SETTINGS),
                        "merged disk members keep native settings"
                    );
                    assert!(
                        members.iter().any(Card::actionable),
                        "merged disk members keep native settings"
                    );
                }
                let (settings, operation) = native.operation();
                assert_eq!(
                    settings,
                    Some(app::DISK_SETTINGS),
                    "merged disk members keep native settings"
                );
                assert!(
                    Rc::ptr_eq(operation.as_ref().unwrap(), native.check.as_ref().unwrap()),
                    "merged disk members keep native settings"
                );
                let (settings, operation) = plugin.operation();
                assert_eq!(settings, None, "merged disk members keep native settings");
                assert!(
                    Rc::ptr_eq(
                        operation.as_ref().unwrap(),
                        plugin.request.as_ref().unwrap()
                    ),
                    "merged disk members keep native settings"
                );
            }) as fn(&str),
        ),
        (
            "requests_follow_each_members_capability",
            (|_scenario: &str| {
                let native = card(Status::Granted, true, false);
                let plugin = card(Status::Required, false, true);
                let rows = catalog::rows(&[native.clone(), plugin.clone()]);
                assert_eq!(
                    rows[0].status,
                    Status::Partial,
                    "requests follow each members capability"
                );
                assert!(
                    !native.actionable(),
                    "requests follow each members capability"
                );
                assert!(
                    plugin.actionable(),
                    "requests follow each members capability"
                );
                assert!(
                    !card(Status::Required, false, false).actionable(),
                    "requests follow each members capability"
                );
                for status in [
                    Status::Remote,
                    Status::Unavailable,
                    Status::Granted,
                    Status::NotNeeded,
                ] {
                    assert!(
                        !card(status, true, true).actionable(),
                        "requests follow each members capability"
                    );
                }
                let remote = card(Status::Remote, true, false);
                let rows = catalog::rows(&[remote, plugin]);
                assert!(
                    rows[0].settings.is_none(),
                    "requests follow each members capability"
                );
            }),
        ),
        (
            "denied_members_open_settings_without_requesting",
            (|_scenario: &str| {
                let mut denied = card(Status::Denied, true, true);
                denied.resource = Resource::Screen;
                let (settings, operation) = denied.operation();
                assert!(
                    settings.is_some(),
                    "denied members open settings without requesting"
                );
                assert!(
                    operation.is_none(),
                    "denied members open settings without requesting"
                );
                denied.settings = None;
                assert!(
                    denied.actionable(),
                    "denied members open settings without requesting"
                );
                assert!(
                    denied.operation().1.is_some(),
                    "denied members open settings without requesting"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
