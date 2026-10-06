use super::*;
use crate::settings::providers::authorization_support as support;
use sailry_protocol::{Effort, conversation::login};
use std::sync::atomic::AtomicU8;

struct Surface(Entity<Workspace>);
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}
fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(
        bounds.center().y < viewport.height,
        "{selector} is outside viewport: {bounds:?}, {viewport:?}"
    );
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[path = "authorization/advanced.rs"]
mod advanced;
#[path = "authorization/cancellation.rs"]
mod cancellation;
#[path = "authorization/reconnection.rs"]
mod reconnection;
#[path = "authorization/recovery.rs"]
mod recovery;

#[gpui::test]
fn configures_an_empty_account(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [Preset::ChatGpt, Preset::Copilot, Preset::CopilotResponses] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let mode = Arc::new(AtomicU8::new(1));
            let server = runtime.block_on(support::server(mode.clone()));
            let node = runtime
                .block_on(Node::start_with_authorization(
                    directory.path().join("node"),
                    &server.endpoint,
                ))
                .unwrap();
            let fixture = Fixture::with_node(remote, runtime, directory, node);
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
            });
            let editor = visual.update(|window, cx| {
                let editor =
                    crate::settings::providers::editor::open(owner.clone(), None, window, cx);
                editor.update(cx, |editor, cx| {
                    editor.select_preset(preset, window, cx);
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Account catalog", window, cx)
                    });
                    assert!(editor.models.is_empty());
                    assert_eq!(editor.source(cx).unwrap_err(), "provider_login_required");
                });
                editor
            });
            settle(visual);
            assert!(visual.debug_bounds("provider-login-0").is_none());
            tap(visual, "provider-connect");
            editor.read_with(visual, |editor, _| {
                assert!(
                    editor.pending || editor.authorization.is_some() || editor.error.is_some(),
                    "connect button must receive the click"
                );
                assert_eq!(editor.error, None);
            });
            wait(visual, |cx| editor.read(cx).authorization.is_some());
            wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            let provider = &providers[0];
            assert_eq!(provider.name, "Account catalog");
            if preset == Preset::ChatGpt {
                fixture.execute(Command::PutCredential {
                    id: "00000000-0000-0000-0000-000000000001".parse().unwrap(),
                    provider: provider.id,
                    expected_revision: 0,
                    secret: Secret::new("isolated-unselected-key".into()),
                    expires_at_ms: None,
                });
                wait(visual, |cx| {
                    editor.read(cx).authorization.as_ref().is_some_and(|login| {
                        matches!(login.read(cx).state(), Some(login::State::Browser { .. }))
                    })
                });
                let authorization = editor.read_with(visual, |editor, cx| {
                    let Some(login::State::Browser {
                        authorization_url, ..
                    }) = editor.authorization.as_ref().unwrap().read(cx).state()
                    else {
                        panic!("browser authorization expected")
                    };
                    authorization_url.clone()
                });
                visual.update(|window, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.name.update(cx, |name, cx| {
                            name.set_value("Unsaved account name", window, cx)
                        })
                    })
                });
                let callback = support::callback(&authorization);
                fixture.runtime.block_on(async {
                    reqwest::get(callback.expose())
                        .await
                        .unwrap()
                        .error_for_status()
                        .unwrap();
                });
            }
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].credential_configured
            });
            wait(visual, |cx| {
                editor.read(cx).configured
                    && !editor.read(cx).pending
                    && !editor.read(cx).models.is_empty()
            });
            if preset == Preset::ChatGpt {
                wait(visual, |cx| {
                    editor.read(cx).account_label.as_deref() == Some("user@example.com")
                });
                let Output::Credentials(credentials) = fixture.execute(Command::ListCredentials)
                else {
                    panic!("credentials expected")
                };
                assert_eq!(credentials.len(), 2);
                assert_eq!(
                    credentials[0].id.to_string(),
                    "00000000-0000-0000-0000-000000000001"
                );
                assert!(!credentials[0].revoked);
                assert_eq!(credentials[0].account_label, None);
                assert_ne!(
                    editor.read_with(visual, |editor, _| editor
                        .provider
                        .as_ref()
                        .unwrap()
                        .credential
                        .as_ref()
                        .unwrap()
                        .id),
                    credentials[0].id
                );
            }
            visual.update(|window, cx| {
                assert!(window.has_active_dialog(cx));
                editor.read_with(cx, |editor, cx| {
                    assert!(!editor.closed);
                    assert_eq!(
                        editor.name.read(cx).value().as_ref(),
                        if preset == Preset::ChatGpt {
                            "Unsaved account name"
                        } else {
                            "Account catalog"
                        }
                    );
                });
            });
            settle(visual);
            tap(visual, "provider-discover");
            wait(visual, |cx| {
                editor.read(cx).probe.is_none() && !editor.read(cx).models.is_empty()
            });
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.models.len(), 2);
                assert_eq!(editor.error, None);
                let model = editor.models[0].value(cx).unwrap();
                assert_eq!((model.context, model.output), (16384, 1024));
                assert!(model.vision && model.reasoning);
                assert_eq!(model.efforts, [Effort::Low, Effort::High]);
                assert_eq!(
                    model.default_effort,
                    if preset == Preset::ChatGpt {
                        Effort::High
                    } else {
                        Effort::Low
                    }
                );
                assert!(model.tools);
                let partial = editor.models[1].value(cx).unwrap();
                assert_eq!((partial.context, partial.output), (32768, 16384));
                assert_eq!(editor.error, None);
            });
            visual.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    editor.models.pop();
                    cx.notify();
                })
            });
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).closed);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].default_model, "fixture");
            assert_eq!(providers[0].models.len(), 1);
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].models.len() == 1
            });
            let editor = visual.update(|window, cx| {
                crate::settings::providers::editor::open(owner.clone(), Some(0), window, cx)
            });
            settle(visual);
            assert!(visual.debug_bounds("provider-validate").is_none());
            assert!(visual.debug_bounds("provider-validation").is_none());
            tap(visual, "provider-discover");
            wait(visual, |cx| editor.read(cx).probe.is_none());
            settle(visual);
            editor.read_with(visual, |editor, _| {
                assert_eq!(editor.error, None);
                assert_eq!(editor.models.len(), 2);
            });
            assert!(visual.debug_bounds("provider-validation").is_none());
            mode.store(2, std::sync::atomic::Ordering::SeqCst);
            tap(visual, "provider-discover");
            wait(visual, |cx| editor.read(cx).error.is_some());
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.error),
                Some("provider_account_unavailable")
            );
            mode.store(3, std::sync::atomic::Ordering::SeqCst);
            let count = server.requests.lock().unwrap().len();
            tap(visual, "provider-discover");
            fixture.runtime.block_on(server.wait_requests(count + 1));
            assert!(editor.read_with(visual, |editor, _| editor.probe.is_some()));
            tap(visual, "provider-cancel");
            editor.read_with(visual, |editor, _| {
                assert!(editor.closed && editor.probe.is_none());
            });
            fixture.runtime.block_on(server.wait_closed(1));
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
