use super::*;
use std::time::Duration;

#[gpui::test]
fn feature_navigation_does_not_capture_keys(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Activity, window, cx))
    });
    cx.simulate_modifiers_change(Keystroke::parse("secondary-w").unwrap().modifiers);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for name in ["workspace", "git", "files", "ssh", "database", "activity"] {
        assert!(
            cx.debug_bounds(Box::leak(format!("menu-hint-{name}").into_boxed_str()))
                .is_none()
        );
    }
    for key in [
        "secondary-w",
        "secondary-g",
        "secondary-s",
        "secondary-d",
        "secondary-1",
        "secondary-2",
    ] {
        cx.simulate_keystrokes(key);
        assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Activity);
    }
    cx.simulate_keystrokes("secondary-,");
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Settings);
}
