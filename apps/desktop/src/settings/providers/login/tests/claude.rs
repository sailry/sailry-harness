use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::oauth::DEFAULT_CLAUDE_AGENT;
use std::collections::BTreeMap;
use url::Url;

fn authorization(login: &Entity<Login>, visual: &mut VisualTestContext) -> String {
    wait(visual, |cx| {
        matches!(login.read(cx).state(), Some(State::Browser { .. }))
    });
    login.read_with(visual, |login, _| match login.state() {
        Some(State::Browser {
            authorization_url, ..
        }) => authorization_url.clone(),
        _ => panic!("browser authorization expected"),
    })
}

fn assert_connected(fixture: &Fixture, login: &Entity<Login>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        login.read(cx).closed && !login.read(cx).pending
    });
    let provider = fixture.provider();
    assert!(provider.credential.is_some());
    assert_eq!(provider.authentication, Authentication::ClaudeOAuth);
    assert_eq!(provider.api, ModelApi::Anthropic);
    assert_eq!(provider.endpoint, "https://api.anthropic.com");
    assert!(provider.oauth.is_none());
    assert!(
        provider
            .models
            .iter()
            .any(|model| model.id == "claude-fixture")
    );
    assert_eq!(
        fixture.state(login.read_with(visual, |login, _| login.request.id)),
        State::Connected
    );
    assert!(login.read_with(visual, |login, cx| login.manual.read(cx).value().is_empty()));
    let requests = fixture.server.requests.lock().unwrap();
    let models = requests
        .iter()
        .find(|request| request.path.starts_with("/v1/models"))
        .unwrap();
    assert_eq!(models.headers["user-agent"], DEFAULT_CLAUDE_AGENT);
    assert_eq!(
        models.headers["authorization"],
        "Bearer sk-ant-oat01-desktop-claude-access"
    );
    assert!(models.headers.contains_key("anthropic-beta"));
    assert!(!models.headers.contains_key("x-api-key"));
}

#[gpui::test]
fn receives_loopback_callback(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = browser::mount(&fixture, cx);
        let login = visual.update(|window, cx| open(owner, 0, window, cx).unwrap());
        let url = authorization(&login, visual);
        let query: BTreeMap<_, _> = Url::parse(&url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert!(query["redirect_uri"].ends_with("/callback"));
        assert!(!query["redirect_uri"].ends_with("/auth/callback"));
        fixture.mode.store(1, Ordering::SeqCst);
        let response = authorize_browser(&login, &fixture, visual);
        assert_connected(&fixture, &login, visual);
        page_result(response, &fixture, visual, "connected");
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn accepts_manual_callback_while_listener_waits(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for shape in ["url", "fragment", "query", "code"] {
            let fixture = Fixture::new(remote);
            fixture.execute(Command::SaveProvider {
                provider: support::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic),
                expected_revision: 0,
                secret: None,
            });
            let (owner, visual) = browser::mount(&fixture, cx);
            let login = visual.update(|window, cx| open(owner, 0, window, cx).unwrap());
            let url = authorization(&login, visual);
            settle_dialog(visual);
            assert!(visual.debug_bounds("provider-login-callback").is_some());
            assert!(visual.debug_bounds("provider-login-continue").is_some());
            assert!(visual.debug_bounds("provider-login-loading").is_none());
            assert!(login.read_with(visual, |login, _| login.receiver.is_some()));
            let callback = support::callback(&url);
            let parsed = Url::parse(callback.expose()).unwrap();
            let query: BTreeMap<_, _> = parsed.query_pairs().into_owned().collect();
            let value = match shape {
                "url" => callback.expose().to_owned(),
                "fragment" => format!("{}#{}", query["code"], query["state"]),
                "query" => parsed.query().unwrap().to_owned(),
                "code" => query["code"].clone(),
                _ => unreachable!(),
            };
            visual.update(|window, cx| {
                let input = login.read(cx).manual.clone();
                input.update(cx, |input, cx| input.focus(window, cx));
            });
            visual.simulate_input(&value);
            fixture.mode.store(1, Ordering::SeqCst);
            tap(visual, "provider-login-continue");
            assert_connected(&fixture, &login, visual);
            assert_eq!(
                fixture
                    .server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/v1/oauth/token")
                    .count(),
                1
            );
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn cancels_without_exchanging_code(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = browser::mount(&fixture, cx);
        let login = visual.update(|window, cx| open(owner, 0, window, cx).unwrap());
        authorization(&login, visual);
        settle_dialog(visual);
        tap(visual, "provider-login-continue");
        assert!(visual.debug_bounds("provider-login-callback").is_some());
        assert!(!login.read_with(visual, |login, _| login.pending));
        browser::close_dialog(visual);
        wait(visual, |cx| {
            !login.read(cx).pending && !login.read(cx).cancelling
        });
        assert_eq!(
            fixture.state(login.read_with(visual, |login, _| login.request.id)),
            State::Cancelled
        );
        assert!(fixture.provider().credential.is_none());
        assert!(
            !fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.path == "/v1/oauth/token")
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
