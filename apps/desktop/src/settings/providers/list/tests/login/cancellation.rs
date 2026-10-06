use super::*;
use std::sync::atomic::Ordering;

#[gpui::test]
fn cancels_pending_device_authorization(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.mode.store(0, Ordering::SeqCst);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let saved = fixture.provider();
        let (owner, visual) = fixture.mount(cx);
        for phase in [0, 1, 2] {
            fixture
                .mode
                .store(if phase == 1 { 2 } else { 0 }, Ordering::SeqCst);
            let attempt = fixture.begin(visual);
            if phase == 1 {
                wait(visual, |_| {
                    matches!(fixture.state(attempt), State::Failed(_))
                });
            }
            if phase == 2 {
                visual.simulate_keystrokes("escape");
                draw(visual);
            } else {
                tap(visual, "provider-login-cancel");
            }
            wait(visual, |_| !fixture.state(attempt).active());
            if phase == 1 {
                assert!(matches!(fixture.state(attempt), State::Failed(_)));
            } else {
                assert_eq!(fixture.state(attempt), State::Cancelled);
            }
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            assert_eq!(fixture.provider(), saved);
            assert!(fixture.provider().credential.is_none());
            draw(visual);
        }
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
        drop(owner);
        fixture.close();
    }
}
