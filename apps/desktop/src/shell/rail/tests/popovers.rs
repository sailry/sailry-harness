use super::*;
use gpui_kit::test::TestWindowExt as _;

#[gpui::test]
fn stock_popovers_preserve_dismissal_and_clamping(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    click(&mut visual, "composer-input");
    let focus = visual.update(|window, cx| window.focused(cx));
    assert!(focus.is_some());
    for (trigger, item, menu) in [
        ("sidebar-host", "sidebar-host-option-0", true),
        ("navigation-more", "more-navigation-files", true),
        ("notifications-open", "notifications-panel", false),
        ("installations-open", "installation-panel", false),
    ] {
        let trigger_bounds = visual.debug_bounds(trigger).unwrap();
        click(&mut visual, trigger);
        assert!(visual.debug_bounds(item).is_some());
        let panel = visual.update(|window, cx| {
            window.render_frame(cx);
            if menu {
                window.find("popup-menu").bounds()
            } else {
                // Kit 0.7.3 does not expose its native Popover frame to queries.
                // Observe the app body without asserting private frame geometry.
                window.find(item).bounds()
            }
        });
        let viewport = visual.update(|window, _| window.viewport_size());
        assert!(panel.left() >= trigger_bounds.right(), "{trigger}");
        if trigger_bounds.center().y - panel.size.height / 2. >= px(8.)
            && trigger_bounds.center().y + panel.size.height / 2. <= viewport.height - px(8.)
        {
            assert!(
                (panel.center().y - trigger_bounds.center().y).abs() <= px(1.),
                "sidebar popover is vertically centered: {trigger}"
            );
        }
        if menu {
            assert!(panel.left() >= px(8.));
            assert!(panel.top() >= px(8.));
            assert!(panel.right() <= viewport.width - px(8.));
            assert!(panel.bottom() <= viewport.height - px(8.));
        }
        assert_ne!(visual.update(|window, cx| window.focused(cx)), focus);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        if menu {
            assert!(visual.debug_bounds(item).is_none());
        } else {
            assert!(visual.update(|window, _| window.try_find(item).is_none()));
        }
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
