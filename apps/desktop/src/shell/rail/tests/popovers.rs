use super::*;
use gpui_kit::test::TestWindowExt as _;

#[gpui::test]
fn stock_popovers_preserve_dismissal_and_clamping(cx: &mut TestAppContext) {
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
        let panel = visual.update(|window, _| window.find("popup-menu").bounds());
        let viewport = visual.update(|window, _| window.viewport_size());
        assert!(panel.left() >= px(8.));
        assert!(panel.top() >= px(8.));
        assert!(panel.right() <= viewport.width - px(8.));
        assert!(panel.bottom() <= viewport.height - px(8.));
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
    let panel = visual.update(|window, _| window.find("popup-menu").bounds());
    assert!(panel.left() >= px(8.));
    assert!(panel.right() <= viewport.width - px(8.));
    assert!(panel.top() >= px(8.));
    assert!(panel.bottom() <= viewport.height - px(8.));
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("sidebar-host-option-0").is_none());
    assert_eq!(shell.read_with(&visual, |shell, _| shell.host), 0);
    assert!(shell.read_with(&visual, |shell, _| shell.live.is_none()));
}
