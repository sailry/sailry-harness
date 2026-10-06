use super::*;

#[gpui::test]
fn cancels_browser_without_losing_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
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
            Root::new(cx.new(|_| Surface(workspace)), window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let editor = visual.update(|window, cx| {
            let editor = crate::settings::providers::editor::open(owner.clone(), None, window, cx);
            editor.update(cx, |editor, cx| {
                editor.select_preset(Preset::ChatGpt, window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Saved registration", window, cx)
                });
            });
            editor
        });
        settle(visual);
        for phase in [0, 1, 2] {
            let close_editor = phase == 2;
            tap(visual, "provider-connect");
            wait(visual, |cx| {
                editor.read(cx).authorization.as_ref().is_some_and(|login| {
                    login.read(cx).active()
                        && matches!(login.read(cx).state(), Some(login::State::Browser { .. }))
                })
            });
            let login = editor.read_with(visual, |editor, _| {
                editor.authorization.as_ref().unwrap().clone()
            });
            let (attempt, authorization) = editor.read_with(visual, |editor, cx| {
                let login = login.read(cx);
                let Some(login::State::Browser {
                    authorization_url, ..
                }) = login.state()
                else {
                    panic!("browser authorization expected")
                };
                assert!(editor.authorizing(cx));
                (login.attempt_id(), authorization_url.clone())
            });
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor
                        .name
                        .update(cx, |input, cx| input.set_value("Unsaved draft", window, cx));
                })
            });
            if phase == 1 {
                mode.store(2, std::sync::atomic::Ordering::SeqCst);
                fixture.runtime.block_on(async {
                    reqwest::get(support::callback(&authorization).expose())
                        .await
                        .unwrap()
                        .error_for_status()
                        .unwrap();
                });
                wait(visual, |cx| {
                    matches!(login.read(cx).state(), Some(login::State::Failed(_)))
                });
            }
            tap(
                visual,
                if close_editor {
                    "provider-cancel"
                } else {
                    "provider-login-cancel"
                },
            );
            wait(visual, |cx| login.read(cx).finished());
            if close_editor {
                assert!(editor.read_with(visual, |editor, _| editor.closed));
                visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
            } else {
                wait(visual, |cx| {
                    editor
                        .read(cx)
                        .authorization
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .finished()
                });
                visual.update(|window, cx| {
                    assert!(window.has_active_dialog(cx));
                    assert_eq!(
                        editor.read(cx).name.read(cx).value().as_ref(),
                        "Unsaved draft"
                    );
                    assert!(!editor.read(cx).closed);
                });
            }
            let client = Client::new(fixture.transport.clone());
            fixture.runtime.block_on(async {
                let mut stream = client.subscribe_login(attempt).await.unwrap();
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let sailry_protocol::Update::ProviderLogin(update) =
                            stream.next().await.unwrap()
                        else {
                            panic!("login update expected")
                        };
                        if !update.state.active() {
                            if phase == 1 {
                                assert!(matches!(update.state, login::State::Failed(_)));
                            } else {
                                assert_eq!(update.state, login::State::Cancelled);
                            }
                            break;
                        }
                    }
                })
                .await
                .unwrap();
                let callback = support::callback(&authorization);
                assert!(
                    reqwest::get(callback.expose()).await.is_err(),
                    "cancelled callback listener must be closed"
                );
            });
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers.len(), 1);
            assert!(providers[0].credential.is_none());
            settle(visual);
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
