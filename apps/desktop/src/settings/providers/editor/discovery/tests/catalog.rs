use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn metadata() -> serde_json::Value {
    json!({"anthropic":{"models":{
        "known":{"id":"known", "limit":{"context":8192,"output":1024},
            "modalities":{"input":["text","image"]}, "tool_call":true, "reasoning":true,
            "reasoning_options":[{"type":"effort","values":["low","medium","high","xhigh"]}]},
        "unknown":{"id":"unknown"},
        "unavailable":{"id":"unavailable", "limit":{"context":4096,"output":128}}
    }}})
}

#[gpui::test]
fn fills_cloud_models(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(metadata())
    }));
    for remote in [false, true] {
        for preset in [
            Preset::AzureOpenAi,
            Preset::AzureAi,
            Preset::BedrockIam,
            Preset::VertexAdc,
        ] {
            let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
            fixture.execute(Command::RefreshModelCatalog);
            let mut editor = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let owner = fixture.owner(window, cx);
                let view = cx.new(|cx| Editor::new(owner, None, window, cx));
                view.update(cx, |editor, cx| {
                    editor.select_preset(preset, window, cx);
                    editor.endpoint.update(cx, |input, cx| {
                        input.set_value("http://127.0.0.1:9/must-not-query", window, cx)
                    });
                    for id in ["known", "deployment-alias"] {
                        editor.add_model(window, cx);
                        editor
                            .models
                            .last_mut()
                            .unwrap()
                            .id
                            .update(cx, |input, cx| input.set_value(id, window, cx));
                    }
                    editor.discover(window, cx);
                });
                editor = Some(view.clone());
                Root::new(view, window, cx)
            });
            let editor = editor.unwrap();
            wait(visual, |cx| editor.read(cx).probe.is_none());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert_eq!(editor.models.len(), 2);
                let known = editor.models[0].value(cx).unwrap();
                assert_eq!((known.context, known.output), (8192, 1024));
                assert!(known.vision && known.tools && known.reasoning);
                assert!(
                    known
                        .efforts
                        .iter()
                        .all(|effort| effort.validate(preset.api(), known.output).is_ok())
                );
                let alias = editor.models[1].value(cx).unwrap();
                assert_eq!((alias.context, alias.output), (200_000, 16_384));
                assert!(!alias.vision && !alias.tools && !alias.reasoning);
                assert!(editor.credential.read(cx).value().is_empty());
            });
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| editor.close(window, cx));
                window.remove_window();
            });
            fixture.close();
        }
    }
}

#[gpui::test]
fn fills_compatible_channels(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(metadata())
    }));
    for preset in [
        Preset::CompatibleChat,
        Preset::CompatibleResponses,
        Preset::CompatibleAnthropic,
        Preset::CompatibleGemini,
    ] {
        let api = preset.api();
        let server = runtime.block_on(Server::start(api, move |path| {
            let expected = if api == ModelApi::Gemini {
                "/v1beta/models"
            } else {
                "/v1/models"
            };
            if path.split('?').next() != Some(expected) {
                return Reply::Raw(
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .into(),
                );
            }
            let entries = if api == ModelApi::Gemini {
                json!({"models":[
                    {"name":"models/known", "supportedGenerationMethods":["generateContent"]},
                    {"name":"models/unknown", "supportedGenerationMethods":["generateContent"]}
                ]})
            } else {
                json!({"data":[{"id":"known"}, {"id":"unknown"}], "has_more":false})
            };
            Reply::Json(entries)
        }));
        for remote in [false, true] {
            let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
            fixture.execute(Command::RefreshModelCatalog);
            let mut editor = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let owner = fixture.owner(window, cx);
                let view = cx.new(|cx| Editor::new(owner, None, window, cx));
                view.update(cx, |editor, cx| {
                    editor.select_preset(preset, window, cx);
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Catalog fixture", window, cx)
                    });
                    editor.endpoint.update(cx, |input, cx| {
                        input.set_value(
                            server
                                .endpoint
                                .trim_end_matches("/v1")
                                .trim_end_matches("/v1beta"),
                            window,
                            cx,
                        )
                    });
                    editor.discover(window, cx);
                });
                editor = Some(view.clone());
                Root::new(view, window, cx)
            });
            let editor = editor.unwrap();
            wait(visual, |cx| editor.read(cx).probe.is_none());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert_eq!(editor.models.len(), 2);
                let model = editor.models[0].value(cx).unwrap();
                assert_eq!((model.context, model.output), (8192, 1024));
                assert!(model.vision && model.tools && model.reasoning);
                let fallback = editor.models[1].value(cx).unwrap();
                assert_eq!((fallback.context, fallback.output), (200_000, 16_384));
                assert!(editor.channel(cx).is_ok());
                assert_eq!(editor.endpoint.read(cx).value().as_ref(), server.endpoint);
                assert_eq!(editor.connection(cx).unwrap().1, server.endpoint);
            });
            visual.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
            wait(visual, |cx| !editor.read(cx).pending);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers.len(), 1);
            assert_eq!(providers[0].endpoint, server.endpoint);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn fills_supported_reference_fields(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(metadata())
    }));
    let provider = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(json!({"data":[
        {"id":"manual"}, {"id":"known", "max_input_tokens":4096}, {"id":"unknown"}
    ], "has_more":false}))
    }));
    for remote in [false, true] {
        let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
        fixture.execute(Command::RefreshModelCatalog);
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = fixture.owner(window, cx);
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            view.update(cx, |editor, cx| {
                editor.select_preset(Preset::Anthropic, window, cx);
                editor.endpoint.update(cx, |input, cx| {
                    input.set_value(&provider.endpoint, window, cx)
                });
                editor.credential.update(cx, |input, cx| {
                    input.set_value("isolated-catalog-key", window, cx)
                });
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("manual", window, cx));
                editor.discover(window, cx);
            });
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.models.len(), 3);
            assert_eq!(editor.default_model, 0);
            assert_eq!(editor.models[0].value(cx).unwrap().context, 200000);
            let known = editor.models[1].value(cx).unwrap();
            assert_eq!((known.context, known.output), (4096, 1024));
            assert!(known.vision && known.tools && known.reasoning);
            assert!(!known.web_search);
            use sailry_protocol::Effort;
            assert_eq!(
                known.efforts,
                [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh]
            );
            assert_eq!(known.default_effort, Effort::Low);
            assert_eq!(editor.models[2].value(cx).unwrap().context, 200000);
            assert_eq!(editor.error, None);
        });
        let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        assert!(providers.is_empty());
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.close(window, cx)));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn refresh_failure_preserves_models(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let failure = Arc::new(AtomicBool::new(false));
    let fail = failure.clone();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, move |_| {
        if fail.load(Ordering::SeqCst) {
            Reply::Json(json!({"invalid":true}))
        } else {
            Reply::Json(metadata())
        }
    }));
    for remote in [false, true] {
        failure.store(false, Ordering::SeqCst);
        let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
        let mut workspace = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = fixture.owner(window, cx);
            view.update(cx, |owner, cx| {
                owner.select(crate::settings::Section::Providers, cx)
            });
            workspace = Some(view.clone());
            Root::new(view, window, cx)
        });
        let workspace = workspace.unwrap();
        wait(visual, |cx| {
            workspace.read(cx).provider_link.as_ref().unwrap().connected
        });
        assert!(visual.debug_bounds("models-dev-refresh").is_none());
        wait(visual, |cx| {
            workspace
                .read(cx)
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .status
                .revision
                == 1
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .refresh
                .is_none()
        });
        assert!(visual.debug_bounds("models-dev-refresh").is_none());
        failure.store(true, Ordering::SeqCst);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("catalog-updated").is_some());
        assert!(
            visual
                .debug_bounds("settings-row-provider_catalog_last_update")
                .is_some()
        );
        let updated_at = workspace.read_with(visual, |owner, _| {
            owner
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .status
                .updated_at_ms
                .unwrap()
        });
        visual.update(|_, cx| {
            workspace.update(cx, |owner, cx| {
                owner.select(crate::settings::Section::General, cx);
                owner.select(crate::settings::Section::Providers, cx);
            })
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .error
                .is_some()
        });
        workspace.read_with(visual, |owner, _| {
            let catalog = &owner.provider_link.as_ref().unwrap().catalog;
            assert_eq!(catalog.status.models, 3);
            assert_eq!(catalog.status.revision, 1);
            assert_eq!(catalog.status.updated_at_ms, Some(updated_at));
            assert_eq!(catalog.error, Some("provider_catalog_failed"));
            assert!(owner.providers.channels.is_empty());
        });
        failure.store(false, Ordering::SeqCst);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("catalog-updated").is_some());
        let button = visual.debug_bounds("models-dev-refresh").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        wait(visual, |cx| {
            workspace
                .read(cx)
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .status
                .revision
                == 2
        });
        assert!(workspace.read_with(visual, |owner, _| {
            owner
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .error
                .is_none()
        }));
        assert!(visual.debug_bounds("models-dev-refresh").is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn isolates_host_refresh(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let held = runtime.block_on(Server::start(ModelApi::Anthropic, |_| Reply::Hold));
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(metadata())
    }));
    for remote in [false, true] {
        let source = Fixture::with_catalog(remote, Some(&held.endpoint));
        let target = Fixture::with_catalog(remote, Some(&catalog.endpoint));
        target.execute(Command::RefreshModelCatalog);
        let mut workspace = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = source.owner(window, cx);
            workspace = Some(view.clone());
            Root::new(view, window, cx)
        });
        let workspace = workspace.unwrap();
        wait(visual, |cx| {
            workspace.read(cx).provider_link.as_ref().unwrap().connected
        });
        visual.update(|_, cx| workspace.update(cx, |owner, cx| owner.refresh_catalog(cx)));
        let count = if remote { 2 } else { 1 };
        runtime.block_on(held.wait_requests(count));
        visual.update(|_, cx| {
            workspace.update(cx, |owner, cx| {
                owner.bind_providers(
                    target.transport.clone(),
                    target.runtime.clone(),
                    "Target".into(),
                    cx,
                )
            })
        });
        runtime.block_on(held.wait_closed(count));
        wait(visual, |cx| {
            workspace
                .read(cx)
                .provider_link
                .as_ref()
                .unwrap()
                .catalog
                .status
                .revision
                == 1
        });
        workspace.read_with(visual, |owner, _| {
            let live = owner.provider_link.as_ref().unwrap();
            assert_eq!(live.binding.client.target(), target.transport.target());
            assert!(live.catalog.error.is_none());
            assert!(live.catalog.refresh.is_none());
        });
        visual.update(|window, _| window.remove_window());
        source.close();
        target.close();
    }
}
