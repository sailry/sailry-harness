use super::*;
use crate::feedback;
use gpui_kit::component::notification::{Notification, NotificationType};

#[gpui::test]
fn toast_feedback_stays_out_of_the_inbox(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    visual.update(|window, cx| {
        feedback::info("Fixture", "Completed", window, cx);
        feedback::error("Fixture", "Failed", window, cx);
        feedback::status(
            window,
            "Saved".into(),
            NotificationType::Success,
            Notification::success("Saved").autohide(false),
            cx,
        );
        assert_eq!(window.notifications(cx).len(), 3);
    });
    feedback::tests::settle(visual);
    assert!(shell.read_with(visual, |shell, _| shell.activity.inbox.notices().is_empty()));
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.activity.inbox.unread()),
        0
    );
    assert!(visual.debug_bounds("notifications-unread").is_none());
    click(visual, "notifications-open");
    assert!(visual.debug_bounds("notifications-panel").is_some());
    assert!(visual.debug_bounds("notification-0").is_none());
    assert!(visual.debug_bounds("feedback-notice-1").is_none());
    visual.update(|window, cx| window.clear_notifications(cx));
    feedback::tests::settle(visual);
    assert!(shell.read_with(visual, |shell, _| shell.activity.inbox.notices().is_empty()));
}

#[gpui::test]
fn notice_and_toast_clearing_are_independent(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = fixture::Fixture::new();
    for index in 0..2 {
        delivery::publish(&fixture, index, false);
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| shell.read(cx).activity.inbox.unread() == 2);
    visual.update(|window, cx| window.clear_notifications(cx));
    feedback::tests::settle(visual);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.activity.inbox.unread()),
        2
    );
    visual.update(|window, cx| {
        feedback::status(
            window,
            "Operation complete".into(),
            NotificationType::Info,
            Notification::info("Operation complete").autohide(false),
            cx,
        )
    });
    feedback::tests::settle(visual);
    let toasts = visual.update(|window, cx| window.notifications(cx));
    assert_eq!(toasts.len(), 1);
    click(visual, "notifications-open");
    let panel = visual.debug_bounds("notifications-panel").unwrap();
    let trigger = visual.debug_bounds("notifications-open").unwrap();
    assert!(panel.size.width <= px(320.));
    assert!(panel.left() > trigger.right());
    let offset = visual.update(|window, _| Shell::rail_popover_offset(window));
    assert!(
        (f32::from(panel.bottom() - trigger.bottom())).abs() <= 1.,
        "notification bottom: panel={panel:?}, trigger={trigger:?}, offset={offset:?}"
    );
    for index in 0..2 {
        let selector = Box::leak(format!("notification-{index}").into_boxed_str());
        let row = visual.debug_bounds(selector).unwrap();
        let label = visual
            .debug_bounds(Box::leak(format!("{selector}-label").into_boxed_str()))
            .unwrap();
        let icon = visual
            .debug_bounds(Box::leak(format!("{selector}-icon").into_boxed_str()))
            .unwrap();
        assert_eq!(row.size.height, px(36.));
        assert!(label.size.height <= px(24.));
        assert!(icon.right() < label.left());
        assert_eq!(label.right(), row.right() - px(8.));
    }
    let handle = visual.update(|window, _| window.window_handle());
    for viewport in [size(px(900.), px(640.)), size(px(680.), px(480.))] {
        visual.simulate_window_resize(handle, viewport);
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let panel = visual.debug_bounds("notifications-panel").unwrap();
        let trigger = visual.debug_bounds("notifications-open").unwrap();
        assert!(panel.left() > trigger.right());
        let offset = visual.update(|window, _| Shell::rail_popover_offset(window));
        assert!(
            (f32::from(panel.bottom() - trigger.bottom())).abs() <= 1.,
            "notification bottom: panel={panel:?}, trigger={trigger:?}, offset={offset:?}, viewport={viewport:?}"
        );
        assert!(panel.top() >= px(0.));
        assert!(panel.right() <= viewport.width);
    }
    click(visual, "notifications-clear");
    wait(visual, |cx| {
        shell.read(cx).activity.inbox.notices().is_empty()
    });
    for index in 0..2 {
        assert!(delivery::snapshot(&fixture, index).notifications.is_empty());
    }
    assert_eq!(visual.update(|window, cx| window.notifications(cx)), toasts);
    assert!(visual.debug_bounds("notifications-unread").is_none());
    assert!(visual.debug_bounds("notification-0").is_none());
    drop(shell);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
