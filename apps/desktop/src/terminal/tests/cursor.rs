use super::*;

pub(super) fn check(cx: &mut VisualTestContext, view: &Entity<View>) {
    let original = view.read_with(cx, |view, _| view.state.clone());
    view.update(cx, |view, _| {
        let snapshot = Arc::make_mut(view.state.snapshot.as_mut().unwrap());
        snapshot.screen.cursor = Some(protocol::Cursor {
            column: 0,
            row: 0,
            at_wide_tail: false,
            style: protocol::CursorStyle::Bar,
            blinking: true,
        });
        view.focused = true;
        view.window_active = true;
        view.cursor_visible = true;
        assert!(view.cursor_focused());
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(530));
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.cursor_visible));
    view.update(cx, |view, _| {
        let mut output = view.state.clone();
        Arc::make_mut(output.snapshot.as_mut().unwrap()).sequence += 1;
        view.update_screen(output);
        assert!(!view.cursor_visible);
    });
    cx.executor().advance_clock(Duration::from_millis(530));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.cursor_visible));
    for (focused, active, connected, blinking) in [
        (false, true, true, true),
        (true, false, true, true),
        (true, true, false, true),
        (true, true, true, false),
    ] {
        view.update(cx, |view, _| {
            view.focused = focused;
            view.window_active = active;
            view.state.connected = connected;
            Arc::make_mut(view.state.snapshot.as_mut().unwrap())
                .screen
                .cursor
                .as_mut()
                .unwrap()
                .blinking = blinking;
            view.cursor_visible = true;
        });
        cx.executor().advance_clock(Duration::from_millis(530));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.cursor_visible));
    }
    view.update(cx, |view, cx| {
        view.focused = true;
        view.window_active = true;
        view.update_screen(original);
        cx.notify();
    });
}
