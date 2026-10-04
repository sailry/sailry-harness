use super::workspace::click;
use super::*;

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn bottom(cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(600.), px(400.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
        ..Default::default()
    });
    redraw(cx);
}

fn host(shell: &Entity<Shell>, id: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(id, window, cx)));
    redraw(cx);
    bottom(cx);
}

fn fill(cx: &mut VisualTestContext, value: &str) {
    click(cx, "tunnel-policy-input");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    redraw(cx);
}

#[gpui::test]
fn edit_cancel_validate_and_save(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    host(&shell, 0, &mut cx);
    click(&mut cx, "tunnel-edit");
    fill(&mut cx, "80");
    click(&mut cx, "tunnel-policy-cancel");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.tunnels[&0].ports.clone()),
        vec![3000, 5173]
    );
    bottom(&mut cx);
    click(&mut cx, "tunnel-edit");
    fill(&mut cx, "0,65536");
    cx.simulate_keystrokes("enter");
    redraw(&mut cx);
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.debug_bounds("tunnel-policy-error").is_none());
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    fill(&mut cx, "8080, 3000 8080");
    click(&mut cx, "tunnel-policy-save");
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.tunnels[&0].ports.clone()),
        vec![3000, 8080]
    );
    bottom(&mut cx);
    click(&mut cx, "tunnel-edit");
    fill(&mut cx, " ");
    cx.simulate_keystrokes("enter");
    redraw(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).workspace.tunnels[&0].ports.is_empty()));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.tunnels[&0].entries.len()),
        2
    );
}

#[gpui::test]
fn bound_host_and_stale_policy(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    host(&shell, 0, &mut cx);
    click(&mut cx, "tunnel-edit");
    fill(&mut cx, "8080");
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(1, window, cx)));
    click(&mut cx, "tunnel-policy-save");
    cx.update(|_, cx| {
        assert_eq!(shell.read(cx).host, 1);
        assert_eq!(shell.read(cx).workspace.tunnels[&0].ports, vec![8080]);
        assert_eq!(shell.read(cx).workspace.tunnels[&1].ports, vec![3000, 5173]);
    });
    bottom(&mut cx);
    assert!(cx.debug_bounds("tunnel-edit").is_none());
    assert!(cx.debug_bounds("tunnel-readonly").is_some());
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.tunnel_editor(1, window, cx)));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    for revoke in [false, true] {
        host(&shell, 0, &mut cx);
        click(&mut cx, "tunnel-edit");
        fill(&mut cx, "9000");
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                let state = shell.workspace.tunnels.get_mut(&0).unwrap();
                if revoke {
                    state.editable = false;
                } else {
                    state.ports = vec![1234];
                }
                cx.notify();
            })
        });
        click(&mut cx, "tunnel-policy-save");
        crate::feedback::tests::shown(&mut cx);
        assert!(cx.debug_bounds("tunnel-policy-error").is_none());
        assert_eq!(
            cx.update(|_, cx| shell.read(cx).workspace.tunnels[&0].ports.clone()),
            vec![1234]
        );
        cx.simulate_keystrokes("escape");
        redraw(&mut cx);
    }
}

#[gpui::test]
fn isolates_close(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations[&(0, 0)]
                .input
                .update(cx, |input, cx| input.set_value("Kept draft", window, cx));
        })
    });
    for id in [0, 1] {
        host(&shell, id, &mut cx);
        click(&mut cx, "tunnel-open-0");
        crate::feedback::tests::shown(&mut cx);
        assert!(!cx.has_pending_prompt());
        assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Host);
        cx.update(|window, cx| window.clear_notifications(cx));
        bottom(&mut cx);
        click(&mut cx, "tunnel-close-0");
        bottom(&mut cx);
        assert!(cx.debug_bounds("tunnel-row-0").is_none());
        assert!(cx.debug_bounds("tunnel-row-1").is_some());
        click(&mut cx, "tunnel-close-1");
        assert!(cx.debug_bounds("tunnels-empty").is_some());
    }
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.workspace.terminals.len(), 2);
        assert_eq!(shell.workspace.sessions.len(), 5);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Kept draft"
        );
    });
}

#[gpui::test]
fn errors_recovery_and_small_window(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        host(&shell, 0, &mut cx);
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                let state = shell.workspace.tunnels.get_mut(&0).unwrap();
                state.loaded = false;
                state.entries.clear();
                state.ports = (1..=120).collect();
                cx.notify();
            })
        });
        redraw(&mut cx);
        bottom(&mut cx);
        assert!(cx.debug_bounds("tunnels-loading").is_some());
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.workspace.tunnels.get_mut(&0).unwrap().error = Some("tunnel_load_error");
                cx.notify();
            })
        });
        redraw(&mut cx);
        bottom(&mut cx);
        crate::feedback::tests::settle(&mut cx);
        click(&mut cx, "tunnels-retry");
        cx.update(|window, cx| window.clear_notifications(cx));
        assert!(cx.debug_bounds("tunnels-error").is_none());
        assert!(cx.debug_bounds("tunnels-empty").is_some());
        bottom(&mut cx);
        for _ in 0..4 {
            if cx.debug_bounds("tunnel-edit").unwrap().top() >= px(0.) {
                break;
            }
            cx.simulate_event(ScrollWheelEvent {
                position: point(px(600.), px(400.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(240.))),
                ..Default::default()
            });
            redraw(&mut cx);
        }
        let edit = cx.debug_bounds("tunnel-edit").unwrap();
        let ports = cx.debug_bounds("tunnel-ports").unwrap();
        assert!(edit.right() <= px(760.));
        assert!(ports.right() <= edit.left());
        assert!((ports.top() - edit.top()).abs() <= px(4.));
        assert!(edit.top() >= px(0.) && edit.bottom() <= px(560.));
        click(&mut cx, "tunnel-edit");
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "tunnel editor should open"
        );
        fill(&mut cx, "invalid");
        click(&mut cx, "tunnel-policy-save");
        let save = cx.debug_bounds("tunnel-policy-save").unwrap();
        let input = cx.debug_bounds("tunnel-policy-input").unwrap();
        assert!(save.bottom() <= px(560.));
        assert!(input.bottom() < save.top());
        click(&mut cx, "tunnel-policy-cancel");
    }
}
