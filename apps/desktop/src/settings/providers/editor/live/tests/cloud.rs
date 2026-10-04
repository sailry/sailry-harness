use super::*;
use sailry_protocol::conversation::cloud::Options;

#[gpui::test]
fn follows_region(cx: &mut TestAppContext) {
    saves_regions(cx, None);
}

#[gpui::test]
fn keeps_override(cx: &mut TestAppContext) {
    saves_regions(cx, Some("https://gateway.example.invalid/cloud"));
}

fn saves_regions(cx: &mut TestAppContext, override_address: Option<&str>) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [
            Preset::Bedrock,
            Preset::BedrockIam,
            Preset::Vertex,
            Preset::VertexAdc,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let node = runtime
                .block_on(Node::start(directory.path().join("node")))
                .unwrap();
            let controller = runtime
                .block_on(Link::controller(
                    directory.path().join("controller"),
                    NetworkScope::default(),
                ))
                .unwrap();
            let address = runtime
                .block_on(
                    controller
                        .handle()
                        .pair(node.link().invite().unwrap().ticket()),
                )
                .unwrap();
            let transport: Arc<dyn Transport> = if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            };
            let client = Client::new(transport.clone());
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                view.update(cx, |workspace, cx| {
                    workspace.bind_providers(
                        transport.clone(),
                        runtime.clone(),
                        "Fixture Node".into(),
                        cx,
                    );
                });
                owner = Some(view.clone());
                Root::new(view, window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| {
                owner.read(cx).provider_link.as_ref().unwrap().connected
            });
            let mut id = None;
            for (index, (region, location)) in [
                ("us-east-1", "global"),
                ("eu-west-1", "europe-west1"),
                ("us-west-2", "global"),
            ]
            .into_iter()
            .enumerate()
            {
                let editor = visual.update(|window, cx| {
                    let editor = cx.new(|cx| Editor::new(owner.clone(), id, window, cx));
                    editor.update(cx, |editor, cx| {
                        if id.is_none() {
                            editor.select_preset(preset, window, cx);
                            editor.name.update(cx, |input, cx| {
                                input.set_value("Fixture channel", window, cx)
                            });
                            if preset.key_auth() {
                                editor.credential.update(cx, |input, cx| {
                                    input.set_value("synthetic-key", window, cx)
                                });
                            }
                            editor.add_model(window, cx);
                            editor.models[0].id.update(cx, |input, cx| {
                                input.set_value("fixture-model", window, cx)
                            });
                            if let Some(endpoint) = override_address {
                                editor
                                    .endpoint
                                    .update(cx, |input, cx| input.set_value(endpoint, window, cx));
                            }
                        }
                        assert_eq!(
                            editor.endpoint.read(cx).value(),
                            override_address.unwrap_or_default()
                        );
                        editor.cloud.project.update(cx, |input, cx| {
                            input.set_value("fixture-project", window, cx)
                        });
                        editor
                            .cloud
                            .region
                            .update(cx, |input, cx| input.set_value(region, window, cx));
                        editor
                            .cloud
                            .location
                            .update(cx, |input, cx| input.set_value(location, window, cx));
                    });
                    editor
                });
                wait(visual, |cx| !editor.read(cx).pending);
                visual.update(|window, cx| {
                    editor.update(cx, |editor, cx| editor.save(window, cx));
                });
                wait(visual, |cx| {
                    !editor.read(cx).pending
                        && owner
                            .read(cx)
                            .provider_link
                            .as_ref()
                            .unwrap()
                            .providers
                            .values()
                            .any(|provider| provider.revision == index as u64 + 1)
                });
                editor.read_with(visual, |editor, _| assert_eq!(editor.error, None));
                id = Some(owner.read_with(visual, |owner, _| owner.providers.channels[0].id));
                let Output::Providers(providers) = runtime
                    .block_on(client.execute(client.prepare(Command::ListProviders)))
                    .unwrap()
                else {
                    panic!("providers expected")
                };
                assert_eq!(providers.len(), 1);
                let provider = &providers[0];
                let (options, endpoint) = if matches!(preset, Preset::Bedrock | Preset::BedrockIam)
                {
                    (
                        Options::Bedrock {
                            region: region.into(),
                        },
                        format!("https://bedrock-runtime.{region}.amazonaws.com"),
                    )
                } else {
                    (
                        Options::Vertex {
                            project: "fixture-project".into(),
                            location: location.into(),
                        },
                        if location == "global" {
                            "https://aiplatform.googleapis.com".into()
                        } else {
                            format!("https://{location}-aiplatform.googleapis.com")
                        },
                    )
                };
                assert_eq!(provider.options.as_ref(), Some(&options));
                assert_eq!(provider.endpoint, override_address.unwrap_or(&endpoint));
                assert_eq!(provider.authentication, preset.authentication());
            }
            visual.update(|window, _| window.remove_window());
            runtime.block_on(controller.close()).unwrap();
            runtime.block_on(node.shutdown()).unwrap();
        }
    }
}
