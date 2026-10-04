use super::*;
use crate::workspace::RuntimeSnapshot;

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn host_snapshot_isolation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations[&(0, 0)].input.update(cx, |input, cx| {
                input.set_value("Preserved draft", window, cx);
            });
            shell.select_host(0, window, cx);
        });
    });
    redraw(&mut cx);
    let baseline = cx.debug_bounds("host-runtime-facts").unwrap();
    let resources = cx.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell.workspace.sessions.len(),
            shell.workspace.terminals.len(),
        )
    });
    for host in [0, 1, 0] {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .workspace
                    .runtimes
                    .insert(host, RuntimeSnapshot::sample(host));
                shell.select_host(host, window, cx);
            });
        });
        redraw(&mut cx);
        assert_eq!(cx.debug_bounds("host-runtime-facts").unwrap(), baseline);
        assert!(cx.debug_bounds("host-runtime-loading").is_none());
        assert!(cx.debug_bounds("host-runtime-error").is_none());
        cx.update(|_, cx| {
            let shell = shell.read(cx);
            assert_eq!(
                shell.workspace.runtimes[&host],
                RuntimeSnapshot::sample(host)
            );
            assert_eq!(
                (
                    shell.workspace.sessions.len(),
                    shell.workspace.terminals.len()
                ),
                resources
            );
        });
    }
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.select_session((0, 0), window, cx));
    });
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 0)].input.read(cx).value()),
        "Preserved draft"
    );
}

#[gpui::test]
fn unavailable_snapshots(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for scene in 0..4 {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let mut snapshot = RuntimeSnapshot::sample(0);
                snapshot.loading = scene == 0;
                snapshot.error = (scene == 1).then(|| "Request failed".into());
                shell.workspace.runtimes.insert(0, snapshot);
                if scene == 3 {
                    shell.workspace.runtimes.remove(&0);
                }
                shell.select_host(0, window, cx);
            });
        });
        redraw(&mut cx);
        assert_eq!(
            cx.debug_bounds("host-runtime-loading").is_some(),
            scene == 0
        );
        assert!(cx.debug_bounds("host-runtime-error").is_none());
        if scene == 1 {
            assert!(!cx.update(|window, cx| window.notifications(cx).is_empty()));
        }
        for key in [
            "host-runtime-host_version",
            "host-runtime-host_system_version",
            "host-runtime-host_timezone",
            "host-runtime-host_uptime",
            "host-runtime-host_protocol",
        ] {
            assert!(cx.debug_bounds(key).is_some());
        }
        if scene < 3 {
            assert_eq!(
                cx.update(|_, cx| shell.read(cx).workspace.runtimes[&0].facts()),
                RuntimeSnapshot::sample(0).facts()
            );
        }
    }
}

#[gpui::test]
fn bounds_long_values_and_toasts(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            shell.update(cx, |shell, cx| {
                let mut snapshot = RuntimeSnapshot::sample(0);
                snapshot.version = Some("long-build-version".repeat(100).into());
                snapshot.system = Some("Long system name ".repeat(100).into());
                snapshot.timezone = Some("Long/Timezone/Name".repeat(100).into());
                snapshot.error = Some("Long request failure ".repeat(100).into());
                shell.workspace.runtimes.insert(0, snapshot);
                shell.select_host(0, window, cx);
            });
        });
        redraw(&mut cx);
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(600.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-280.))),
            ..Default::default()
        });
        redraw(&mut cx);
        let facts = cx.debug_bounds("host-runtime-facts").unwrap();
        for key in [
            "host-runtime-host_version",
            "host-runtime-host_system_version",
            "host-runtime-host_timezone",
            "host-runtime-host_uptime",
            "host-runtime-host_protocol",
        ] {
            let value = cx.debug_bounds(key).unwrap();
            assert!(value.right() <= facts.right());
            assert!(value.right() <= px(760.));
            assert!(value.size.height < px(30.));
        }
        assert!(facts.size.height < px(160.));
        let error = cx.debug_bounds("error-toast-detail").unwrap();
        assert!(error.size.height <= px(96.));
        let before = facts;
        cx.simulate_event(ScrollWheelEvent {
            position: error.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            ..Default::default()
        });
        redraw(&mut cx);
        assert_eq!(cx.debug_bounds("host-runtime-facts").unwrap(), before);
        assert!(cx.debug_bounds("host-runtime-error").is_none());
    }
}
