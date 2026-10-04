use super::*;

#[gpui::test]
fn refreshes_expired_codes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for api in [ModelApi::Responses, ModelApi::ChatCompletions] {
        for remote in [false, true] {
            let fixture = Fixture::new(remote);
            fixture.mode.store(4, Ordering::SeqCst);
            fixture.execute(Command::SaveProvider {
                provider: support::provider(Authentication::Copilot, api),
                expected_revision: 0,
                secret: None,
            });
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                view.update(cx, |view, cx| {
                    view.bind_providers(
                        fixture.transport.clone(),
                        fixture.runtime.clone(),
                        "Authorization Node".into(),
                        cx,
                    )
                });
                owner = Some(view.clone());
                Root::new(cx.new(|_| Surface(view)), window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
            let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
            wait(visual, |cx| {
                matches!(
                    login.read(cx).state(),
                    Some(State::Failed(Fault {
                        code: ErrorCode::Expired,
                        ..
                    }))
                )
            });
            settle_dialog(visual);
            assert!(visual.debug_bounds("provider-login-code-expired").is_some());
            assert!(visual.debug_bounds("provider-login-refresh").is_some());
            for absent in [
                "provider-login-code",
                "provider-login-open",
                "provider-login-cancel",
                "provider-login-retry",
            ] {
                assert!(visual.debug_bounds(absent).is_none(), "unexpected {absent}");
            }
            let first = login.read_with(visual, |login, _| login.request.id);
            visual.update(|_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()))
            });
            tap(visual, "provider-login-code-expired");
            visual.update(|_, cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some("unchanged")
                )
            });
            fixture.mode.store(1, Ordering::SeqCst);
            tap(visual, "provider-login-refresh");
            wait(visual, |cx| {
                matches!(login.read(cx).state(), Some(State::Connected))
            });
            login.read_with(visual, |login, _| {
                assert_ne!(login.request.id, first);
                assert!(!login.expired());
                assert_eq!(login.error, None);
            });
            assert!(fixture.provider().credential.is_some());
            assert_eq!(
                fixture
                    .server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/login/device/code")
                    .count(),
                2
            );
            tap(visual, "provider-login-cancel");
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[test]
fn does_not_treat_denial_as_expiration() {
    assert_eq!(
        error_key(&Fault::new(ErrorCode::Expired, "Expired fixture")),
        "provider_login_expired"
    );
    assert_eq!(
        error_key(&Fault::new(ErrorCode::PermissionDenied, "Denied fixture")),
        "provider_login_failed"
    );
}
