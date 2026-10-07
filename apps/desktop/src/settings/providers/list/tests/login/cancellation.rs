use super::*;
use std::sync::atomic::Ordering;

#[gpui::test]
fn cancels_browser_and_releases_callback(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.mode.store(1, Ordering::SeqCst);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let saved = fixture.provider();
        let (owner, visual) = fixture.mount(cx);
        for phase in [0, 1, 2] {
            let (attempt, authorization) = fixture.begin(visual);
            let response = if phase == 1 {
                fixture.mode.store(2, Ordering::SeqCst);
                let response = fixture.callback(&authorization);
                wait(visual, |_| {
                    matches!(fixture.state(attempt), State::Failed(_))
                });
                Some(response)
            } else {
                None
            };
            if phase == 2 {
                visual.simulate_keystrokes("escape");
                draw(visual);
            } else {
                close_dialog(visual);
            }
            wait(visual, |_| !fixture.state(attempt).active());
            if phase == 1 {
                assert!(matches!(fixture.state(attempt), State::Failed(_)));
            } else {
                assert_eq!(fixture.state(attempt), State::Cancelled);
            }
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            if let Some(response) = response {
                fixture.page_result(response, visual, "failed");
            }
            fixture.runtime.block_on(async {
                assert!(
                    reqwest::get(support::callback(&authorization).expose())
                        .await
                        .is_err(),
                    "cancelled callback listener must be closed"
                );
            });
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
