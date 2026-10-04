use super::*;
use gpui_kit::component::scroll::ScrollbarHandle;

pub(super) fn check(cx: &mut VisualTestContext, view: &Entity<View>, root: &std::path::Path) {
    std::fs::write(
        root.join("links.sh"),
        "printf '\\033]8;;https://ghostty.org\\007LINK_END\\033]8;;\\007 plain\\n'\n",
    )
    .unwrap();
    paste(cx, "/bin/sh ./links.sh");
    cx.simulate_keystrokes("enter");
    wait(cx, view, |view| {
        view.state
            .snapshot
            .as_ref()
            .unwrap()
            .screen
            .rows
            .iter()
            .flat_map(|row| &row.spans)
            .any(|span| span.hyperlink.as_deref() == Some("https://ghostty.org"))
    });
    let (last_cell, after) = view.read_with(cx, |view, _| {
        let screen = &view.state.snapshot.as_ref().unwrap().screen;
        let (row, span) = screen
            .scrollback
            .iter()
            .chain(&screen.rows)
            .enumerate()
            .find_map(|(row, line)| {
                line.spans
                    .iter()
                    .find(|span| span.hyperlink.is_some())
                    .map(|span| (row, span))
            })
            .unwrap();
        let y = view.metrics.bounds.top()
            + view.scroll.offset().y
            + view.metrics.cell.height * (row as f32 + 0.5);
        let end = view.metrics.bounds.left()
            + view.metrics.cell.width * (span.column + span.columns) as f32;
        (
            point(end - view.metrics.cell.width * 0.1, y),
            point(end + view.metrics.cell.width * 0.1, y),
        )
    });
    let active = Modifiers {
        platform: cfg!(target_os = "macos"),
        control: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    cx.simulate_mouse_move(last_cell, None, Modifiers::default());
    cx.update(|window, cx| assert_eq!(view.read(cx).pointer_cursor(window), CursorStyle::IBeam));
    cx.simulate_modifiers_change(active);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(
            view.read(cx).pointer_cursor(window),
            CursorStyle::PointingHand
        );
    });
    cx.simulate_mouse_move(after, None, active);
    cx.update(|window, cx| assert_eq!(view.read(cx).pointer_cursor(window), CursorStyle::IBeam));
    cx.simulate_mouse_move(last_cell, None, active);
    cx.simulate_click(last_cell, active);
    assert_eq!(cx.opened_url().as_deref(), Some("https://ghostty.org"));
    cx.simulate_modifiers_change(Modifiers::default());
    cx.update(|window, cx| assert_eq!(view.read(cx).pointer_cursor(window), CursorStyle::IBeam));
}
