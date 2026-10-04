use super::workspace::click;
use super::*;

fn redraw(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

fn open(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(0, window, cx)));
    redraw(cx);
    click(cx, "toggle-details");
    assert!(cx.debug_bounds("host-metrics").is_some());
}

#[gpui::test]
fn host_scope_and_lifetime(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    let old = cx.update(|_, cx| shell.read(cx).host_metrics.as_ref().unwrap().downgrade());
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(1, window, cx)));
    redraw(&mut cx);
    assert!(old.upgrade().is_none());
    let remote = cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(shell.side_resource.is_none());
        let panel = shell.host_metrics.as_ref().unwrap();
        assert_eq!(panel.read(cx).host, 1);
        assert_eq!(panel.read(cx).snapshot.as_ref().unwrap().cpu, 24.);
        panel.downgrade()
    });
    click(&mut cx, "toggle-details");
    assert!(remote.upgrade().is_none());
    assert!(cx.debug_bounds("host-metrics").is_none());
    click(&mut cx, "toggle-details");
    let reopened = cx.update(|_, cx| shell.read(cx).host_metrics.as_ref().unwrap().downgrade());
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.navigate(Page::Files, window, cx)));
    redraw(&mut cx);
    assert!(reopened.upgrade().is_none());
    assert!(cx.update(|_, cx| shell.read(cx).layout.panel_open[Page::Host.panel_index()]));
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.navigate(Page::Host, window, cx)));
    redraw(&mut cx);
    assert!(cx.debug_bounds("host-metrics").is_some());
    click(&mut cx, "toggle-details");
    assert!(cx.update(|_, cx| shell.read(cx).host_metrics.is_none()));
}

#[gpui::test]
fn loading_error_and_empty_states(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    let panel = cx.update(|_, cx| shell.read(cx).host_metrics.as_ref().unwrap().clone());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            panel.update(cx, |panel, cx| {
                panel.snapshot = None;
                panel.error = None;
                cx.notify();
            });
        });
        redraw(&mut cx);
        assert!(cx.debug_bounds("metrics-loading").is_some());
        assert!(cx.debug_bounds("metrics-data").is_none());
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.error = Some(tr("metrics_error"));
                cx.notify();
            })
        });
        redraw(&mut cx);
        assert!(!cx.update(|window, cx| window.notifications(cx).is_empty()));
        assert!(cx.debug_bounds("metrics-error").is_none());
        assert!(cx.debug_bounds("metrics-loading").is_none());
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                let mut snapshot = crate::workspace::MetricsSnapshot::sample(0);
                snapshot.memory_total = 0;
                snapshot.interval = std::time::Duration::ZERO;
                snapshot.disks.clear();
                snapshot.processes.clear();
                panel.snapshot = Some(snapshot);
                panel.error = None;
                cx.notify();
            })
        });
        redraw(&mut cx);
        assert!(cx.debug_bounds("metrics-data").is_some());
        assert!(cx.debug_bounds("metrics-error").is_none());
        assert!(cx.debug_bounds("metrics-disk-0").is_none());
        assert!(cx.debug_bounds("metrics-process-0").is_none());
        assert!(cx.debug_bounds("close-metrics").is_none());
        assert!(cx.debug_bounds("toggle-details").is_some());
    }
}

#[gpui::test]
fn preserves_workspace_geometry(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.files.tabs.open(1);
            shell.git.tabs.open(1);
            shell.conversations[&(0, 0)].input.update(cx, |input, cx| {
                input.set_value("Kept draft", window, cx);
            });
            shell.layout.panel_width[Page::Host.panel_index()] = 300.;
        })
    });
    open(&shell, &mut cx);
    let handle = cx.update(|window, _| window.window_handle());
    let before = cx.debug_bounds("host-metrics").unwrap();
    let start = point(before.left(), px(400.));
    cx.simulate_mouse_move(start, None, Modifiers::default());
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    for step in 1..=4 {
        cx.simulate_mouse_move(
            point(start.x - px(step as f32 * 15.), start.y),
            MouseButton::Left,
            Modifiers::default(),
        );
        redraw(&mut cx);
    }
    cx.simulate_mouse_up(
        point(start.x - px(60.), start.y),
        MouseButton::Left,
        Modifiers::default(),
    );
    redraw(&mut cx);
    let resized = cx.debug_bounds("host-metrics").unwrap();
    assert!(resized.size.width > before.size.width + px(40.));
    let width = cx.update(|_, cx| shell.read(cx).layout.panel_width[Page::Host.panel_index()]);
    cx.simulate_window_resize(handle, size(px(760. + RAIL_WIDTH), px(560.)));
    redraw(&mut cx);
    assert!(cx.debug_bounds("host-metrics").is_none());
    assert!(cx.update(|_, cx| shell.read(cx).layout.panel_open[Page::Host.panel_index()]));
    cx.simulate_window_resize(handle, size(px(1280.), px(820.)));
    redraw(&mut cx);
    assert!((cx.debug_bounds("host-metrics").unwrap().size.width - px(width)).abs() <= px(2.));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert_eq!(shell.layout.panel_width[0], 560.);
            assert_eq!(shell.files.tabs.open, vec![0]);
            // Selecting a host resets independent file/Git context, as before.
            shell.files.tabs.open(1);
            shell.git.tabs.open(1);
            shell.navigate(Page::Conversation, window, cx);
            assert_eq!(shell.files.tabs.open, vec![0, 1]);
            assert_eq!(shell.git.tabs.open, vec![0, 1]);
            assert_eq!(
                shell.conversations[&(0, 0)].input.read(cx).value(),
                "Kept draft"
            );
        })
    });
}

#[gpui::test]
fn long_list_toggle(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    let handle = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.layout.panel_width[Page::Host.panel_index()] = 260.;
            shell.layout.sidebar_open = false;
            shell
                .host_metrics
                .as_ref()
                .unwrap()
                .update(cx, |panel, cx| {
                    let snapshot = panel.snapshot.as_mut().unwrap();
                    snapshot.disks[0].mount = "/preview/long-volume-name".repeat(20).into();
                    snapshot.disks = vec![snapshot.disks[0].clone(); 12];
                    snapshot.processes[0].name = "Long preview process name ".repeat(20).into();
                    snapshot.processes = vec![snapshot.processes[0].clone(); 30];
                    cx.notify();
                });
            cx.notify();
        });
        window.window_handle()
    });
    cx.simulate_window_resize(handle, size(px(760. + RAIL_WIDTH), px(560.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        redraw(&mut cx);
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(650.), px(400.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            ..Default::default()
        });
        redraw(&mut cx);
        let panel = cx.debug_bounds("host-metrics").unwrap();
        let tail = cx.debug_bounds("metrics-process-29").unwrap();
        let toggle = cx.debug_bounds("toggle-details").unwrap();
        assert!(tail.right() <= panel.right());
        assert!(tail.left() >= panel.left());
        assert!(tail.bottom() <= px(560.));
        assert!(tail.top() >= toggle.bottom());
        assert!(toggle.bottom() <= px(HEADER_HEIGHT * 2.));
    }
    click(&mut cx, "toggle-details");
    assert!(cx.debug_bounds("host-metrics").is_none());
}
