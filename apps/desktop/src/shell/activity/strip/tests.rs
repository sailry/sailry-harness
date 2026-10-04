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
