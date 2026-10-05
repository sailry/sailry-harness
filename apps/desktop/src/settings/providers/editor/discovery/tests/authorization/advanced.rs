use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::oauth::Options;
use std::sync::atomic::Ordering;

fn replace(visual: &mut VisualTestContext, selector: &'static str, value: &str) {
    tap(visual, selector);
    visual.update(|window, cx| {
        window.dispatch_action(Box::new(gpui_kit::component::input::SelectAll), cx);
        window.dispatch_action(Box::new(gpui_kit::component::input::Backspace), cx);
    });
    visual.simulate_input(value);
    visual.run_until_parked();
}

fn custom(preset: Preset) -> Options {
    match preset {
        Preset::ChatGpt => Options::ChatGpt {
            catalog_version: "0.160.1".into(),
            user_agent: "Fixture/9.3".into(),
        },
        Preset::Copilot | Preset::CopilotResponses => Options::Copilot {
            user_agent: "Fixture/9.3".into(),
            editor_version: "vscode/2.3.4".into(),
            editor_plugin_version: "copilot-chat/1.2.3".into(),
        },
        _ => unreachable!(),
    }
}

fn fill(visual: &mut VisualTestContext, options: &Options) {
    replace(visual, "provider-oauth-user-agent", options.user_agent());
    match options {
        Options::ChatGpt {
            catalog_version, ..
        } => {
            replace(visual, "provider-oauth-catalog-version", catalog_version);
        }
        Options::Copilot {
            editor_version,
            editor_plugin_version,
            ..
        } => {
            replace(visual, "provider-oauth-editor-version", editor_version);
            replace(
                visual,
                "provider-oauth-editor-plugin-version",
                editor_plugin_version,
            );
        }
    }
}

fn parameters(request: &crate::discovery_fixture::Request, options: &Options) {
    assert_eq!(request.headers["user-agent"], options.user_agent());
    match options {
        Options::ChatGpt {
            catalog_version, ..
        } => {
            assert!(
                request
                    .path
                    .contains(&format!("client_version={catalog_version}"))
            );
            assert_eq!(request.headers["chatgpt-account-id"], "desktop-fixture");
        }
        Options::Copilot {
            editor_version,
            editor_plugin_version,
            ..
        } => {
            assert_eq!(request.headers["editor-version"], *editor_version);
            assert_eq!(
                request.headers["editor-plugin-version"],
                *editor_plugin_version
            );
        }
    }
}

fn connected(fixture: &Fixture, preset: Preset) -> Provider {
    let client = Client::new(fixture.transport.clone());
    fixture.runtime.block_on(support::connect(
        &client,
        support::provider(preset.authentication(), preset.api()),
    ))
}

fn mounted<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
) -> (Entity<Workspace>, Entity<Editor>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = fixture.owner(window, cx);
        owner = Some(workspace.clone());
        let surface = cx.new(|_| Surface(workspace));
        Root::new(surface, window, cx)
    });
    let owner = owner.unwrap();
    wait(visual, |cx| {
        owner.read(cx).provider_link.as_ref().unwrap().connected
            && owner.read(cx).providers.channels.len() == 1
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1100.)));
    let editor = visual.update(|window, cx| {
        crate::settings::providers::editor::open(owner.clone(), Some(0), window, cx)
    });
    settle(visual);
    (owner, editor, visual)
}

#[gpui::test]
fn queries_and_saves_custom_parameters(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [Preset::ChatGpt, Preset::Copilot, Preset::CopilotResponses] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let server = runtime.block_on(support::server(Arc::new(AtomicU8::new(1))));
            let node = runtime
                .block_on(Node::start_with_authorization(
                    directory.path().join("node"),
                    &server.endpoint,
                ))
                .unwrap();
            let fixture = Fixture::with_node(remote, runtime, directory, node);
            let saved = connected(&fixture, preset);
            let (owner, editor, visual) = mounted(cx, &fixture);
            assert!(visual.debug_bounds("provider-oauth-user-agent").is_none());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(
                    editor.oauth.options(preset, cx).unwrap(),
                    Options::defaults(preset.authentication())
                );
            });
            tap(visual, "provider-oauth-advanced");
            let draft = custom(preset);
            fill(visual, &draft);
            editor.read_with(visual, |editor, cx| {
                assert_eq!(
                    editor.source(cx).unwrap(),
                    discovery::Source::Saved {
                        provider: saved.id,
                        expected_revision: saved.revision,
                        oauth: Some(draft.clone()),
                    }
                );
            });
            tap(visual, "provider-oauth-advanced");
            let before = server.requests.lock().unwrap().len();
            tap(visual, "provider-discover");
            wait(visual, |cx| {
                editor.read(cx).probe.is_none() && !editor.read(cx).models.is_empty()
            });
            editor.read_with(visual, |editor, _| assert_eq!(editor.error, None));
            let records = server.requests.lock().unwrap();
            let request = records[before..]
                .iter()
                .find(|request| {
                    request.path.starts_with("/models")
                        && request.headers.get("user-agent").map(String::as_str)
                            == Some(draft.user_agent())
                })
                .expect("custom model request");
            parameters(request, &draft);
            drop(records);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].oauth, saved.oauth);
            assert_eq!(providers[0].revision, saved.revision);
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).closed);
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].oauth.as_ref() == Some(&draft)
            });
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].oauth.as_ref(), Some(&draft));
            assert_eq!(providers[0].credential, saved.credential);
            let count = server.requests.lock().unwrap().len();
            let reopened = visual.update(|window, cx| {
                crate::settings::providers::editor::open(owner.clone(), Some(0), window, cx)
            });
            settle(visual);
            reopened.read_with(visual, |editor, cx| {
                assert_eq!(
                    editor.oauth.options(preset, cx).unwrap().as_ref(),
                    Some(&draft)
                )
            });
            assert!(visual.debug_bounds("provider-oauth-user-agent").is_none());
            assert_eq!(server.requests.lock().unwrap().len(), count);
            tap(visual, "provider-cancel");
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn rejects_changed_discovery_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [Preset::ChatGpt, Preset::Copilot, Preset::CopilotResponses] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let mode = Arc::new(AtomicU8::new(1));
            let gate = Arc::new(tokio::sync::Notify::new());
            let server = runtime.block_on(support::server_with_gate(mode.clone(), gate.clone()));
            let node = runtime
                .block_on(Node::start_with_authorization(
                    directory.path().join("node"),
                    &server.endpoint,
                ))
                .unwrap();
            let fixture = Fixture::with_node(remote, runtime, directory, node);
            let saved = connected(&fixture, preset);
            let (_, editor, visual) = mounted(cx, &fixture);
            tap(visual, "provider-oauth-advanced");
            let draft = custom(preset);
            fill(visual, &draft);
            tap(visual, "provider-oauth-advanced");
            let models = editor.read_with(visual, |editor, cx| {
                editor
                    .models
                    .iter()
                    .map(|draft| draft.value(cx).unwrap())
                    .collect::<Vec<_>>()
            });
            let before = server.requests.lock().unwrap().len();
            mode.store(3, Ordering::SeqCst);
            tap(visual, "provider-discover");
            fixture.runtime.block_on(server.wait_requests(before + 1));
            assert!(editor.read_with(visual, |editor, _| editor.probe.is_some()));
            tap(visual, "provider-oauth-advanced");
            replace(visual, "provider-oauth-user-agent", "Changed/2.0");
            tap(visual, "provider-oauth-advanced");
            mode.store(1, Ordering::SeqCst);
            gate.notify_one();
            wait(visual, |cx| editor.read(cx).probe.is_none());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, Some("provider_query_changed"));
                assert_eq!(
                    editor
                        .models
                        .iter()
                        .map(|draft| draft.value(cx).unwrap())
                        .collect::<Vec<_>>(),
                    models
                );
            });
            visual.update(|window, cx| {
                assert_eq!(
                    crate::feedback::tests::summary(window, cx),
                    tr("provider_query_changed")
                )
            });
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].oauth, saved.oauth);
            assert_eq!(providers[0].revision, saved.revision);
            tap(visual, "provider-discover");
            wait(visual, |cx| editor.read(cx).probe.is_none());
            editor.read_with(visual, |editor, _| assert_eq!(editor.error, None));
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .rev()
                    .find(|request| request.path.starts_with("/models"))
                    .unwrap()
                    .headers["user-agent"],
                "Changed/2.0"
            );
            tap(visual, "provider-cancel");
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn retains_invalid_drafts_for_retry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [Preset::ChatGpt, Preset::Copilot, Preset::CopilotResponses] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let server = runtime.block_on(support::server(Arc::new(AtomicU8::new(1))));
            let node = runtime
                .block_on(Node::start_with_authorization(
                    directory.path().join("node"),
                    &server.endpoint,
                ))
                .unwrap();
            let fixture = Fixture::with_node(remote, runtime, directory, node);
            let saved = connected(&fixture, preset);
            let (_, editor, visual) = mounted(cx, &fixture);
            tap(visual, "provider-oauth-advanced");
            let mut draft = custom(preset);
            fill(visual, &draft);
            replace(visual, "provider-oauth-user-agent", "");
            let requests = server.requests.lock().unwrap().len();
            tap(visual, "provider-save");
            editor.read_with(visual, |editor, cx| {
                assert!(!editor.closed && !editor.pending);
                assert_eq!(editor.error, Some("provider_configuration_invalid"));
                assert_eq!(
                    editor.oauth.options(preset, cx).unwrap_err(),
                    "provider_configuration_invalid"
                );
            });
            assert!(visual.debug_bounds("provider-oauth-user-agent").is_some());
            visual.update(|window, cx| {
                assert_eq!(
                    crate::feedback::tests::summary(window, cx),
                    tr("provider_configuration_invalid")
                )
            });
            tap(visual, "provider-discover");
            assert!(editor.read_with(visual, |editor, _| editor.probe.is_none()));
            assert_eq!(server.requests.lock().unwrap().len(), requests);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].revision, saved.revision);
            assert_eq!(providers[0].oauth, saved.oauth);

            // Typing without clearing proves the failed save retained the empty draft.
            tap(visual, "provider-oauth-user-agent");
            visual.simulate_input("Retry/4.2");
            match &mut draft {
                Options::ChatGpt { user_agent, .. } | Options::Copilot { user_agent, .. } => {
                    *user_agent = "Retry/4.2".into()
                }
            }
            editor.read_with(visual, |editor, cx| {
                assert_eq!(
                    editor.oauth.options(preset, cx).unwrap().as_ref(),
                    Some(&draft)
                )
            });
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).closed);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].oauth.as_ref(), Some(&draft));
            assert_eq!(providers[0].credential, saved.credential);
            assert_eq!(server.requests.lock().unwrap().len(), requests);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
