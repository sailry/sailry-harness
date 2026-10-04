use super::*;
use crate::activity::fixture::Fixture;
use gpui_kit::{ScrollDelta, ScrollWheelEvent};
use sailry_client::Client;
use sailry_protocol::{Command, Output, SessionId};
use std::time::Instant;

fn wait(visual: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "sidebar snapshot deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn scrollbar_drags_from_the_scrolled_viewport(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .is_some_and(|live| live.hosts.len() == 2)
    });
    let window = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(window, size(px(1280.), px(900.)));
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node, cx);
            });
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.node == node)
        });
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let Output::Session(unassigned) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(fixture.sessions[index].config.clone()),
            })))
            .unwrap()
        else {
            panic!("unassigned session expected")
        };
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == unassigned.id)
                })
        });
        // Populate the real sidebar composition without creating Agent work.
        let sessions: Vec<_> = (0..32)
            .map(|row| {
                let mut session = if row < 16 {
                    &fixture.sessions[index]
                } else {
                    &unassigned
                }
                .clone();
                session.id = SessionId::new();
                session.activity.title = format!("Scrollbar row {row}");
                session.activity.queued = 1;
                session
            })
            .collect();
        visual.update(|window, cx| {
            cx.global_mut::<crate::preferences::Preferences>()
                .data
                .recent = Some(
                sessions
                    .iter()
                    .map(|session| crate::preferences::recent::Visit {
                        node,
                        session: session.id,
                    })
                    .collect(),
            );
            shell.update(cx, |shell, cx| {
                let snapshot = shell.live.as_mut().unwrap().view.snapshot.as_mut().unwrap();
                snapshot.cursor += 1000;
                snapshot.sessions.extend(sessions);
                shell.sidebar.recent_open = true;
                shell.sidebar.active_closed = false;
                shell.sidebar.projects_closed = false;
                shell.sidebar.unassigned_closed.remove(&node);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        for (selector, handle) in shell.read_with(visual, |shell, _| {
            [
                (
                    "sidebar-unassigned-viewport",
                    shell.sidebar.unassigned_scroll.clone(),
                ),
                (
                    "sidebar-projects-viewport",
                    shell.sidebar.projects_scroll.clone(),
                ),
                ("sidebar-active", shell.sidebar.active_scroll.clone()),
                (
                    "sidebar-recent-viewport",
                    shell.sidebar.recent_scroll.clone(),
                ),
            ]
        }) {
            // Each group gets its normal expanded viewport while its peers
            // are collapsed, so the native thumb has room to be dragged.
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.sidebar.active_closed = selector != "sidebar-active";
                    shell.sidebar.projects_closed = selector != "sidebar-projects-viewport";
                    shell.sidebar.recent_open = selector == "sidebar-recent-viewport";
                    if selector == "sidebar-unassigned-viewport" {
                        shell.sidebar.unassigned_closed.remove(&node);
                    } else {
                        shell.sidebar.unassigned_closed.insert(node);
                    }
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            let viewport = visual.debug_bounds(selector).unwrap();
            assert!(viewport.size.height > px(48.), "{selector}");
            assert_eq!(handle.bounds(), viewport, "{selector}");
            visual.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
                ..Default::default()
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(handle.offset().y < px(0.), "{selector} must overflow");
            assert_eq!(handle.offset().y, -handle.max_offset().y, "{selector}");
            assert_eq!(handle.bounds(), viewport);
            let start = point(viewport.right() - px(10.), viewport.bottom() - px(10.));
            let end = point(start.x, viewport.top() - px(100.));
            visual.simulate_mouse_move(start, None, Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
            visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(handle.offset().y, px(0.), "{selector}, node={index}");
            assert_eq!(handle.bounds(), viewport);
        }
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
