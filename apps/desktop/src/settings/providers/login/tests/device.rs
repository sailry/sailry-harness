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
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        settle_dialog(visual);
        let authorization = login.read_with(visual, |login, _| {
            let Some(State::Pending {
                verification_url, ..
            }) = login.state()
            else {
                panic!("device authorization expected")
            };
            verification_url.clone()
        });
        assert_eq!(visual.opened_url().as_deref(), Some(authorization.as_str()));
        assert!(visual.debug_bounds("provider-login-loading").is_none());
        assert!(visual.debug_bounds("provider-login-code").is_some());
        assert!(visual.debug_bounds("provider-login-waiting").is_none());
        assert!(visual.debug_bounds("provider-editor").is_none());
        assert!(
            !fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| { request.path == "/oauth/token" })
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
fn preserves_card_across_states(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let fixture = Fixture::new(false);
    let binding = Binding {
        client: Arc::new(Client::new(fixture.transport.clone())),
        runtime: fixture.runtime.clone(),
        label: "Authorization Node".into(),
    };
    let mut login = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| {
            Login::new(
                binding,
                support::provider(Authentication::ChatGpt, ModelApi::Responses),
            )
        });
        login = Some(view.clone());
        Root::new(view, window, cx)
    });
    let login = login.unwrap();
    draw(visual);
    let size = visual
        .debug_bounds("provider-login-code-card")
        .unwrap()
        .size;
    for (state, pending, selector, loading) in [
        (None, false, "provider-login-code-placeholder", false),
        (None, true, "provider-login-code-placeholder", true),
        (
            Some(State::Starting),
            false,
            "provider-login-code-placeholder",
            true,
        ),
        (
            Some(State::Failed(Fault::new(
                ErrorCode::PermissionDenied,
                "Denied fixture",
            ))),
            false,
            "provider-login-code-placeholder",
            false,
        ),
        (
            Some(State::Pending {
                verification_url: "https://auth.openai.com/codex/device".into(),
                user_code: "SAIL-1234".into(),
                expires_at_ms: u64::MAX,
            }),
            false,
            "provider-login-code",
            false,
        ),
        (
            Some(State::Failed(Fault::new(
                ErrorCode::Expired,
                "Expired fixture",
            ))),
            false,
            "provider-login-code-expired",
            false,
        ),
        (
            Some(State::Cancelled),
            false,
            "provider-login-code-placeholder",
            false,
        ),
    ] {
        login.update(visual, |login, cx| {
            login.pending = pending;
            login.error = match &state {
                Some(State::Failed(error)) => Some(error_key(error)),
                _ => None,
            };
            login.view.update = state.map(|state| sailry_protocol::conversation::login::Update {
                attempt: Attempt {
                    id: login.request.id,
                    provider: login.provider.id,
                },
                revision: 1,
                state,
                model_error: None,
            });
            cx.notify();
        });
        draw(visual);
        assert_eq!(
            visual
                .debug_bounds("provider-login-code-card")
                .unwrap()
                .size,
            size
        );
        assert!(
            visual.debug_bounds(selector).is_some(),
            "missing {selector}"
        );
        assert_eq!(
            visual.debug_bounds("provider-login-loading").is_some(),
            loading
        );
        visual.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()));
        });
        tap(visual, selector);
        visual.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(if selector == "provider-login-code" {
                    "SAIL-1234"
                } else {
                    "unchanged"
                }),
            );
        });
    }
    assert!(fixture.server.requests.lock().unwrap().is_empty());
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
