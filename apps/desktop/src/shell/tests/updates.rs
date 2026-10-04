use super::workspace::click;
use super::*;
use crate::workspace::UpdateSnapshot;

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

fn open(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(1, window, cx)));
    redraw(cx);
    click(cx, "host-update");
}

#[gpui::test]
fn target_and_close_preserve_status(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(0, window, cx)));
    redraw(&mut cx);
    assert!(cx.debug_bounds("host-update").is_none());
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.host_update(0, window, cx)));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    open(&shell, &mut cx);
    cx.simulate_keystrokes("enter");
    redraw(&mut cx);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.updates[&1].phase),
        UpdateSnapshot::sample(0).phase
    );
    click(&mut cx, "host-update-action");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.updates[&1].phase),
        UpdateSnapshot::sample(3).phase
    );
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(0, window, cx)));
    click(&mut cx, "host-update-action");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.updates[&1].phase),
        UpdateSnapshot::sample(6).phase
    );
    assert_eq!(cx.update(|_, cx| shell.read(cx).host), 0);
    click(&mut cx, "host-update-close");
    assert!(cx.debug_bounds("host-update-dialog").is_none());
    open(&shell, &mut cx);
    click(&mut cx, "host-update-action");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.updates[&1].phase),
        UpdateSnapshot::sample(8).phase
    );
    assert!(cx.debug_bounds("host-update-action").is_none());
    cx.simulate_keystrokes("escape");
    redraw(&mut cx);
    open(&shell, &mut cx);
    assert!(cx.debug_bounds("host-update-action").is_none());
    assert!(cx.update(|_, cx| !shell.read(cx).workspace.updates.contains_key(&0)));
}

#[gpui::test]
fn snapshots_update_open_dialog(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    for scene in 0..13 {
        let snapshot = UpdateSnapshot::sample(scene);
        let action = snapshot.primary().is_some();
        let progress = snapshot.progress.is_some();
        let error = snapshot.error.is_some();
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.workspace.updates.insert(1, snapshot);
                cx.notify();
            })
        });
        redraw(&mut cx);
        assert_eq!(
            cx.debug_bounds("host-update-action").is_some(),
            action,
            "scene {scene}"
        );
        assert_eq!(cx.debug_bounds("host-update-progress").is_some(), progress);
        assert!(cx.debug_bounds("host-update-error").is_none());
        if error {
            assert!(!cx.update(|window, cx| window.notifications(cx).is_empty()));
        }
    }
    click(&mut cx, "host-update-action");
    assert!(cx.debug_bounds("host-update-error").is_none());
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.workspace.updates.remove(&1);
            cx.notify();
        })
    });
    redraw(&mut cx);
    assert!(cx.debug_bounds("host-update-action").is_none());
    click(&mut cx, "host-update-close");
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn keeps_actions_stable_after_failure(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        open(&shell, &mut cx);
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                let mut snapshot = UpdateSnapshot::sample(11);
                snapshot.current = "long-preview-version".repeat(40).into();
                snapshot.note = Some("Long unavailable reason ".repeat(120).into());
                snapshot.error = Some("Long request failure ".repeat(120).into());
                snapshot.progress = Some(255);
                shell.workspace.updates.insert(1, snapshot);
                cx.notify();
            })
        });
        redraw(&mut cx);
        std::thread::sleep(std::time::Duration::from_millis(300));
        redraw(&mut cx);
        let close = cx.debug_bounds("host-update-close").unwrap();
        let action = cx.debug_bounds("host-update-action").unwrap();
        assert!(close.bottom() <= px(560.));
        assert!(action.right() <= px(760.));
        assert!(close.right() <= action.left());
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(400.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            ..Default::default()
        });
        redraw(&mut cx);
        assert!(cx.debug_bounds("host-update-error").is_none());
        assert!(!cx.update(|window, cx| window.notifications(cx).is_empty()));
        assert!(cx.debug_bounds("error-toast-detail").unwrap().size.height <= px(96.));
        assert_eq!(cx.debug_bounds("host-update-close").unwrap(), close);
        click(&mut cx, "host-update-close");
    }
}
