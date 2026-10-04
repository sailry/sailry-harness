use super::*;
use gpui_kit::component::WindowExt;
use std::time::{Duration, Instant};

pub(super) mod observed;

#[track_caller]
pub(super) fn settle(
    cx: &mut VisualTestContext,
    shell: &Entity<Shell>,
    predicate: impl Fn(&Shell, &App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if shell.read_with(cx, |shell, cx| predicate(shell, cx)) {
            break;
        }
        assert!(Instant::now() < deadline, "file view update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        // Dialog geometry uses a wall-clock slide animation, not the executor timer.
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        cx.run_until_parked();
    }
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}
