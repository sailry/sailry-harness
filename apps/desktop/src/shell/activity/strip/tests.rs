use super::*;
use core::prelude::v1::test;

#[test]
fn rotation_pulses_once_then_pauses_each_second() {
    let animation = rotation();
    assert_eq!(animation.duration, Duration::from_secs(1));
    assert!(!animation.oneshot);
    for (phase, rotation) in [(0., 0.), (0.1125, 0.15625), (0.225, 0.5), (0.3375, 0.84375)] {
        assert!(((animation.easing)(phase) - rotation).abs() < 1e-6);
    }
    for phase in [0.45, 0.5, 0.75, 1.] {
        assert_eq!((animation.easing)(phase), 1.);
    }
}

struct Host(Lane);

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        avatar_icon("session", self.0, cx)
    }
}

#[gpui::test]
fn animates_running_when_enabled(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let handle = cx.open_window(size(px(100.), px(100.)), |_, _| Host(Lane::Idle));
    cx.run_until_parked();
    for reduced in [false, true] {
        cx.update(|cx| cx.set_reduce_motion(reduced));
        for state in [
            Lane::Running,
            Lane::Waiting,
            Lane::Running,
            Lane::Completed,
            Lane::Failed,
            Lane::Idle,
        ] {
            handle
                .update(cx, |host, _, cx| {
                    host.0 = state;
                    cx.notify();
                })
                .unwrap();
            let frames = cx
                .update_window(handle.into(), |_, window, cx| {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx)
                })
                .unwrap();
            assert_eq!(frames > 0, state == Lane::Running && !reduced);
        }
    }
    handle
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
}

struct Strip {
    shell: Entity<Shell>,
    snapshot: Snapshot,
    width: Pixels,
}

impl Render for Strip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.shell.update(cx, |shell, cx| {
            let Some(live) = shell.live.as_mut() else {
                return div().into_any_element();
            };
            live.view.snapshot = Some(self.snapshot.clone());
            shell
                .activity_strip(self.width, cx)
                .unwrap_or_else(|| div().into_any_element())
        })
    }
}

#[gpui::test]
fn spaces_actions_and_overflow(cx: &mut TestAppContext) {
    use sailry_client::Client;
    use sailry_protocol::{Command, Output, SessionId, TurnId, conversation::*};

    cx.executor().allow_parking();
    let fixture = crate::activity::fixture::Fixture::new();
    let client = Client::new(fixture.nodes[0].local());
    let Output::Snapshot(snapshot) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected");
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let (handle, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        Root::new(
            cx.new(|_| Strip {
                shell,
                snapshot: snapshot.clone(),
                width: px(700.),
            }),
            window,
            cx,
        )
    });
    for (width, count, slots) in [(700., 3, 3), (700., 4, 3), (440., 4, 2)] {
        let mut scene = snapshot.clone();
        scene.cursor = u64::MAX;
        scene.sessions = (0..count)
            .map(|index| {
                let mut session = fixture.sessions[0].clone();
                session.id = SessionId::new();
                session.activity.attention.unread = true;
                session.activity.run = Some(Run {
                    worktree: session.worktree,
                    turn: TurnId::new(),
                    kind: RunKind::Task,
                    session: session.id,
                    sequence: index,
                    revision: 1,
                    status: Status::Completed,
                    error: None,
                    started_ms: Some(0),
                    finished_ms: Some(1),
                    origin: None,
                });
                session
            })
            .collect();
        let ids: Vec<_> = scene.sessions.iter().map(|session| session.id).collect();
        visual.update(|_, cx| {
            let root = handle.read(cx).view().clone();
            let strip = root.downcast::<Strip>().unwrap();
            strip.update(cx, |strip, cx| {
                strip.snapshot = scene;
                strip.width = px(width);
                cx.notify();
            });
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let visible = if count > slots { slots - 1 } else { count };
        let mut targets: Vec<_> = ids
            .iter()
            .take(visible as usize)
            .map(|id| {
                let selector =
                    Box::leak(format!("header-session-{:?}-{id}", snapshot.node).into_boxed_str());
                visual.debug_bounds(selector).unwrap()
            })
            .collect();
        if count > slots {
            targets.push(
                visual
                    .debug_bounds("header-activity_completed-more")
                    .unwrap(),
            );
        }
        for target in &targets {
            assert_eq!(target.size, size(px(28.), px(28.)));
        }
        for pair in targets.windows(2) {
            assert!(pair[1].left() - pair[0].right() >= px(4.));
        }
        if count > slots {
            visual.simulate_click(targets.last().unwrap().center(), Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("header-activity-list").is_some());
            for id in &ids {
                let selector = Box::leak(
                    format!("header-activity-row-{:?}-{id}", snapshot.node).into_boxed_str(),
                );
                assert!(visual.debug_bounds(selector).is_some());
            }
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
        }
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
