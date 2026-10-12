use super::*;

#[gpui::test]
fn bottom_alignment_preserves_dismissal_and_clamping(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    click(&mut visual, "composer-input");
    let focus = visual.update(|window, cx| window.focused(cx));
    assert!(focus.is_some());
    for (trigger, item) in [
        ("sidebar-host", "sidebar-host-option-0"),
        ("navigation-more", "more-navigation-files"),
    ] {
        click(&mut visual, trigger);
        assert!(visual.debug_bounds(item).is_some());
        let panel = visual
            .debug_bounds("popup-menu")
            .expect("native rail menu surface");
        let trigger = visual.debug_bounds(trigger).unwrap();
        let rail = visual.debug_bounds("shell-feature-rail").unwrap();
        let offset = visual.update(|window, _| Shell::rail_popover_offset(window));
        assert!(panel.left() >= rail.right());
        assert!(panel.top() >= px(8.));
        assert!(
            f32::from(panel.bottom() - trigger.bottom()).abs() <= 1.,
            "rail menu bottom: panel={panel:?}, trigger={trigger:?}, offset={offset:?}"
        );
        assert_ne!(visual.update(|window, cx| window.focused(cx)), focus);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(item).is_none());
        assert_eq!(visual.update(|window, cx| window.focused(cx)), focus);
        assert_eq!(
            shell.read_with(&visual, |shell, _| shell.page),
            Page::Conversation
        );
    }

    let viewport = size(px(220.), px(480.));
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, viewport);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    click(&mut visual, "sidebar-host");
    let panel = visual
        .debug_bounds("popup-menu")
        .expect("clamped native rail menu surface");
    let trigger = visual.debug_bounds("sidebar-host").unwrap();
    assert!(panel.left() >= px(8.));
    assert!(panel.left() < visual.debug_bounds("shell-feature-rail").unwrap().right());
    assert!(panel.right() <= viewport.width - px(8.));
    assert!(panel.top() >= px(8.));
    assert!(panel.bottom() <= viewport.height - px(8.));
    assert!(f32::from(panel.bottom() - trigger.bottom()).abs() <= 1.);
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("sidebar-host-option-0").is_none());
    assert_eq!(shell.read_with(&visual, |shell, _| shell.host), 0);
    assert!(shell.read_with(&visual, |shell, _| shell.live.is_none()));
}
