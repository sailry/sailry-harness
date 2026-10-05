use super::*;

#[gpui::test]
fn closes_on_catalog_failure(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for mode in [5, 6] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            for remote in [false, true] {
                let fixture = Fixture::new(remote);
                fixture.mode.store(mode, Ordering::SeqCst);
                fixture.execute(Command::SaveProvider {
                    provider: support::provider(authentication, api),
                    expected_revision: 0,
                    secret: None,
                });
                let mut owner = None;
                let (_, visual) = cx.add_window_view(|window, cx| {
                    let workspace = cx.new(|cx| Workspace::new(window, cx));
                    workspace.update(cx, |workspace, cx| {
                        workspace.bind_providers(
                            fixture.transport.clone(),
                            fixture.runtime.clone(),
                            "Authorization Node".into(),
                            cx,
                        );
                        workspace.select(crate::settings::Section::Providers, cx);
                    });
                    owner = Some(workspace.clone());
                    Root::new(cx.new(|_| Surface(workspace)), window, cx)
                });
                let owner = owner.unwrap();
                wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
                let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
                wait(visual, |cx| login.read(cx).closed);
                login.read_with(visual, |login, _| {
                    assert_eq!(login.state(), Some(&State::Connected));
                    let error = login
                        .view
                        .update
                        .as_ref()
                        .unwrap()
                        .model_error
                        .as_ref()
                        .unwrap();
                    assert_eq!(error.code, ErrorCode::Unavailable);
                    assert_eq!(
                        error.message,
                        if mode == 6 {
                            "account model catalog is empty"
                        } else {
                            "provider returned HTTP 503"
                        }
                    );
                });
                visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
                crate::feedback::tests::shown(visual);
                assert_eq!(
                    visual.update(crate::feedback::tests::summary),
                    tr("provider_login_models_failed")
                );
                wait(visual, |cx| {
                    owner.read(cx).providers.channels[0].credential_configured
                });
                let provider = fixture.provider();
                assert!(provider.models.is_empty());
                let credential = provider.credential.unwrap();
                draw(visual);
                assert!(visual.debug_bounds("provider-logout-0").is_none());
                fixture.mode.store(1, Ordering::SeqCst);
                tap(visual, "provider-login-0");
                wait(visual, |cx| {
                    owner.read(cx).providers.channels[0].models.len() == 2
                });
                visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
                assert_eq!(fixture.provider().credential, Some(credential));
                crate::feedback::tests::shown(visual);
                assert_eq!(
                    visual.update(crate::feedback::tests::summary),
                    tr("provider_login_connected")
                );
                visual.update(|window, _| window.remove_window());
                fixture.close();
            }
        }
    }
}
