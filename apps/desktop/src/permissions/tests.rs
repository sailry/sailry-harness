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
fn merged_disk_members_keep_native_settings() {
    let native = card(Status::Required, true, false);
    let plugin = card(Status::Required, false, true);
    for members in [
        vec![native.clone(), plugin.clone()],
        vec![plugin.clone(), native.clone()],
    ] {
        let rows = catalog::rows(&members);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, Status::Required);
        assert_eq!(rows[0].settings, Some(app::DISK_SETTINGS));
        assert!(members.iter().any(Card::actionable));
    }
    let (settings, operation) = native.operation();
    assert_eq!(settings, Some(app::DISK_SETTINGS));
    assert!(Rc::ptr_eq(
        operation.as_ref().unwrap(),
        native.check.as_ref().unwrap()
    ));
    let (settings, operation) = plugin.operation();
    assert_eq!(settings, None);
    assert!(Rc::ptr_eq(
        operation.as_ref().unwrap(),
        plugin.request.as_ref().unwrap()
    ));
}

#[test]
fn requests_follow_each_members_capability() {
    let native = card(Status::Granted, true, false);
    let plugin = card(Status::Required, false, true);
    let rows = catalog::rows(&[native.clone(), plugin.clone()]);
    assert_eq!(rows[0].status, Status::Partial);
    assert!(!native.actionable());
    assert!(plugin.actionable());
    assert!(!card(Status::Required, false, false).actionable());
    for status in [
        Status::Remote,
        Status::Unavailable,
        Status::Granted,
        Status::NotNeeded,
    ] {
        assert!(!card(status, true, true).actionable());
    }
    let remote = card(Status::Remote, true, false);
    let rows = catalog::rows(&[remote, plugin]);
    assert!(rows[0].settings.is_none());
}

#[test]
fn denied_members_open_settings_without_requesting() {
    let mut denied = card(Status::Denied, true, true);
    denied.resource = Resource::Screen;
    let (settings, operation) = denied.operation();
    assert!(settings.is_some());
    assert!(operation.is_none());
    denied.settings = None;
    assert!(denied.actionable());
    assert!(denied.operation().1.is_some());
}
