use super::*;

fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| {
            view.bind_providers(
                fixture.transport.clone(),
                fixture.runtime.clone(),
                "Authorization Node".into(),
                cx,
            );
            view.select(crate::settings::Section::Providers, cx);
        });
        owner = Some(view.clone());
        Root::new(cx.new(|_| Surface(view)), window, cx)
    });
    let owner = owner.unwrap();
    wait(visual, |cx| {
        owner.read(cx).provider_link.as_ref().unwrap().connected
            && owner.read(cx).providers.channels.len() == 1
    });
    (owner, visual)
}

#[gpui::test]
fn opens_once_and_waits_without_loading(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = mount(&fixture, cx);
        let login = visual.update(|window, cx| open(owner, 0, window, cx).unwrap());
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Browser { .. }))
        });
        settle_dialog(visual);
        let authorization = login.read_with(visual, |login, _| {
            let Some(State::Browser {
                authorization_url, ..
            }) = login.state()
            else {
                panic!("browser authorization expected")
            };
            authorization_url.clone()
        });
        assert_eq!(visual.opened_url().as_deref(), Some(authorization.as_str()));
        assert!(visual.debug_bounds("provider-login-loading").is_none());
        let waiting = visual.debug_bounds("provider-login-waiting").unwrap();
        assert!(waiting.size.height <= visual.update(|window, _| window.rem_size() * 3.));
        assert!(visual.debug_bounds("provider-login-code").is_none());
        assert!(visual.debug_bounds("provider-editor").is_none());
        assert!(
            !fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| { request.path == "/api/accounts/oauth/token" })
        );
        // Observer repeats must not steal focus back to the browser.
        visual.update(|_, cx| {
            cx.open_url("https://example.com/other-tab");
            login.update(cx, |login, cx| login.open_browser(&authorization, cx));
        });
        assert_eq!(
            visual.opened_url().as_deref(),
            Some("https://example.com/other-tab")
        );
        tap(visual, "provider-login-open");
        assert_eq!(visual.opened_url().as_deref(), Some(authorization.as_str()));
        tap(visual, "provider-login-cancel");
        wait(visual, |cx| {
            !login.read(cx).pending && !login.read(cx).cancelling
        });
        assert_eq!(
            fixture.state(login.read_with(visual, |login, _| login.request.id)),
            State::Cancelled
        );
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn loads_only_during_exchange(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = mount(&fixture, cx);
        let login = visual.update(|window, cx| open(owner, 0, window, cx).unwrap());
        authorize_browser(&login, &fixture, visual);
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Exchanging))
        });
        settle_dialog(visual);
        assert!(visual.debug_bounds("provider-login-loading").is_some());
        assert!(visual.debug_bounds("provider-login-waiting").is_none());
        assert!(visual.debug_bounds("provider-login-open").is_none());
        tap(visual, "provider-login-cancel");
        wait(visual, |cx| {
            !login.read(cx).pending && !login.read(cx).cancelling
        });
        assert_eq!(
            fixture.state(login.read_with(visual, |login, _| login.request.id)),
            State::Cancelled
        );
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
