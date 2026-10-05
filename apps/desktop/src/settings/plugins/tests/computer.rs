use super::*;
use sailry_protocol::computer::{Permission, Permissions};

fn permissions(fixture: &Fixture, local: bool) {
    *fixture.transport.computer.lock().unwrap() = Some(Permissions {
        node: fixture.node.id(),
        platform: "macos".into(),
        local,
        screen_capture: Some(false),
        accessibility: Some(false),
    });
}

fn open(owner: &Entity<Workspace>, visual: &mut VisualTestContext) {
    owner.update(visual, |owner, cx| {
        owner.open_plugin_settings("computer", cx)
    });
    shown_settings(owner, visual, "computer-permissions", true);
}

#[gpui::test]
fn explicit_requests_and_status_refresh(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    permissions(&fixture, true);
    let (owner, visual) = fixture.mount(cx);
    open(&owner, visual);
    shown_settings(&owner, visual, "computer-authorize-screen", true);
    shown_settings(&owner, visual, "computer-authorize-accessibility", true);
    shown_settings(
        &owner,
        visual,
        "settings-icon-computer_screen_capture",
        true,
    );
    shown_settings(&owner, visual, "settings-icon-computer_accessibility", true);
    shown(visual, "settings-group-computer_device", false);
    shown(visual, "plugin-settings-enabled", true);
    assert!(
        !fixture
            .public_packages(false)
            .iter()
            .find(|package| package.name == "computer")
            .unwrap()
            .enabled
    );
    assert!(
        fixture
            .transport
            .permission_requests
            .lock()
            .unwrap()
            .is_empty()
    );
    // Script IDs are not native debug selectors. Click the real Kit button in
    // the first permission row, relative to its trailing header control.
    draw(visual);
    let refresh = visual.debug_bounds("computer-permissions-refresh").unwrap();
    visual.simulate_click(
        point(refresh.right() - px(40.), refresh.bottom() + px(36.)),
        Modifiers::default(),
    );
    shown(visual, "permissions-modal", true);
    assert!(
        fixture
            .transport
            .permission_requests
            .lock()
            .unwrap()
            .is_empty()
    );
    tap(visual, "permission_screen-request");
    wait(visual, |_| {
        *fixture.transport.permission_requests.lock().unwrap() == [Permission::ScreenCapture]
    });
    // Returning from the OS prompt does not imply permission was granted.
    shown_settings(
        &owner,
        visual,
        "computer-screen-computer_permission_missing",
        false,
    );
    shown_settings(&owner, visual, "computer-authorize-screen", true);
    shown(visual, "permission_screen-permission_required", true);
    visual.deactivate_window();
    {
        let mut permissions = fixture.transport.computer.lock().unwrap();
        let permissions = permissions.as_mut().unwrap();
        permissions.screen_capture = Some(true);
        permissions.accessibility = Some(true);
    }
    visual.update(|window, _| window.activate_window());
    shown_settings(
        &owner,
        visual,
        "computer-screen-computer_permission_granted",
        true,
    );
    shown_settings(&owner, visual, "computer-authorize-screen", false);
    shown_settings(&owner, visual, "computer-authorize-accessibility", false);
    assert_eq!(
        *fixture.transport.permission_requests.lock().unwrap(),
        [Permission::ScreenCapture]
    );
    fixture.close(visual);
}

#[gpui::test]
fn wrong_device_recovery(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        permissions(&fixture, !remote);
        fixture
            .transport
            .computer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .node = fixture.other.id();
        let (owner, visual) = fixture.mount(cx);
        open(&owner, visual);
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            "Check failed"
        );
        shown_settings(
            &owner,
            visual,
            "computer-screen-computer_permission_unknown",
            true,
        );
        shown_settings(
            &owner,
            visual,
            "computer-screen-computer_permissions_failed",
            false,
        );
        shown_settings(&owner, visual, "computer-authorize-screen", false);
        permissions(&fixture, !remote);
        tap(visual, "computer-permissions-refresh");
        shown_settings(
            &owner,
            visual,
            if remote {
                "computer_permissions_remote"
            } else {
                "computer-authorize-screen"
            },
            true,
        );
        let original = owner.read_with(visual, |owner, _| {
            owner.plugin_settings_panel().unwrap().downgrade()
        });
        fixture.bind(&owner, visual, true);
        shown_settings(&owner, visual, "computer-permissions", true);
        wait(visual, |cx| {
            owner.read(cx).plugin_settings_panel().is_some_and(|panel| {
                panel.downgrade() != original && panel.read(cx).resource_active()
            })
        });
        wait(visual, |_| original.upgrade().is_none());
        tap(visual, "plugin-settings-enabled");
        wait(visual, |_| {
            fixture
                .public_packages(true)
                .iter()
                .any(|package| package.name == "computer" && package.enabled)
        });
        assert!(
            !fixture
                .public_packages(false)
                .iter()
                .find(|package| package.name == "computer")
                .unwrap()
                .enabled
        );
        assert!(
            fixture
                .transport
                .permission_requests
                .lock()
                .unwrap()
                .is_empty()
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn cancelling_authorization_preserves_current_status(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    permissions(&fixture, true);
    let (owner, visual) = fixture.mount(cx);
    open(&owner, visual);
    draw(visual);
    let refresh = visual.debug_bounds("computer-permissions-refresh").unwrap();
    visual.simulate_click(
        point(refresh.right() - px(40.), refresh.bottom() + px(36.)),
        Modifiers::default(),
    );
    shown(visual, "permissions-modal", true);
    tap(visual, "permissions-cancel");
    shown(visual, "permissions-modal", false);
    shown_settings(&owner, visual, "computer-authorize-screen", true);
    shown_settings(
        &owner,
        visual,
        "computer-screen-computer_permission_missing",
        false,
    );
    // The unchanged missing-access value renders its available action, not a second status label.
    assert_eq!(
        fixture
            .transport
            .computer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .screen_capture,
        Some(false)
    );
    assert!(
        fixture
            .transport
            .permission_requests
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
    fixture.close(visual);
}

#[gpui::test]
fn remote_status_has_no_local_actions(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(true);
    permissions(&fixture, false);
    let (owner, visual) = fixture.mount(cx);
    open(&owner, visual);
    shown_settings(&owner, visual, "computer_permissions_remote", true);
    shown_settings(&owner, visual, "computer-authorize-screen", false);
    shown_settings(&owner, visual, "computer-authorize-accessibility", false);
    shown_settings(
        &owner,
        visual,
        "computer-screen-computer_permission_missing",
        true,
    );
    tap(visual, "computer-permissions-refresh");
    shown_settings(
        &owner,
        visual,
        "computer-screen-computer_permission_missing",
        true,
    );
    assert!(
        fixture
            .transport
            .permission_requests
            .lock()
            .unwrap()
            .is_empty()
    );
    fixture.close(visual);
}
