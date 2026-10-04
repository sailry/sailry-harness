use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::conversation::ModelApi;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

mod cloud;
mod keys;

struct Surface(Entity<Workspace>);

impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "provider update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn reveals_saved_family(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
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
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport.clone(),
                    runtime.clone(),
                    "Fixture Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Providers, cx);
            });
            owner = Some(workspace.clone());
            let surface = cx.new(|_| Surface(workspace));
            Root::new(surface, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let editor = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
            editor.update(cx, |editor, cx| {
                editor.select_preset(
                    Preset::Hosted(super::super::super::vendors::Vendor::DeepSeek),
                    window,
                    cx,
                );
                editor.name.update(cx, |input, cx| {
                    input.set_value("Fixture channel", window, cx)
                });
                editor
                    .credential
                    .update(cx, |input, cx| input.set_value("synthetic-key", window, cx));
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("fixture-model", window, cx));
                editor.save(window, cx);
            });
            editor
        });
        wait(visual, |cx| {
            !editor.read(cx).pending && owner.read(cx).providers.channels.len() == 1
        });
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.error, None);
            assert_eq!(owner.read(cx).providers.category, editor.preset.category());
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let tabs = visual.debug_bounds("provider-families").unwrap();
        assert!(visual.debug_bounds("provider_openai").is_some());
        assert!(visual.debug_bounds("provider-families-empty").is_none());
        let selected = visual.debug_bounds("provider_other").unwrap();
        assert!(selected.left() >= tabs.left() && selected.right() <= tabs.right());
        let id = owner.read_with(visual, |owner, _| owner.providers.channels[0].id);
        assert_eq!(id, 0);
        let remove = visual.debug_bounds("provider-delete-0").unwrap();
        visual.simulate_click(remove.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        crate::prompts::tests::answer(visual, "settings_delete");
        wait(visual, |cx| owner.read(cx).providers.channels.is_empty());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("provider-families-empty").is_none());
        assert!(visual.debug_bounds("provider-families").is_some());
        visual.update(|window, _| window.remove_window());
        runtime.block_on(controller.close()).unwrap();
        runtime.block_on(node.shutdown()).unwrap();
    }
}

#[gpui::test]
fn preserves_target_and_draft(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for (preset, api, endpoint) in [
        (
            Preset::OpenCodeGo,
            ModelApi::OpenCodeGo,
            "https://opencode.ai/zen/go/v1",
        ),
        (
            Preset::OpenCodeZen,
            ModelApi::OpenCodeZen,
            "https://opencode.ai/zen/v1",
        ),
        (
            Preset::CompatibleChat,
            ModelApi::ChatCompletions,
            "http://127.0.0.1:12345/v1",
        ),
        (
            Preset::Anthropic,
            ModelApi::Anthropic,
            "https://api.anthropic.com",
        ),
        (
            Preset::CompatibleAnthropic,
            ModelApi::Anthropic,
            "http://127.0.0.1:12345/anthropic",
        ),
        (
            Preset::CompatibleGemini,
            ModelApi::Gemini,
            "http://127.0.0.1:12345/gemini/v1beta",
        ),
        (
            Preset::Gemini,
            ModelApi::Gemini,
            "https://generativelanguage.googleapis.com/v1beta",
        ),
        (
            Preset::ChatGpt,
            ModelApi::Responses,
            "https://chatgpt.com/backend-api/codex",
        ),
        (
            Preset::Copilot,
            ModelApi::ChatCompletions,
            "https://api.githubcopilot.com",
        ),
        (
            Preset::CopilotResponses,
            ModelApi::Responses,
            "https://api.githubcopilot.com",
        ),
        (
            Preset::AzureOpenAi,
            ModelApi::AzureOpenAi,
            "https://fixture.openai.azure.com",
        ),
        (
            Preset::AzureAi,
            ModelApi::AzureAi,
            "https://fixture.services.ai.azure.com/models",
        ),
        (
            Preset::Bedrock,
            ModelApi::Bedrock,
            "https://bedrock-runtime.us-east-1.amazonaws.com",
        ),
        (
            Preset::BedrockIam,
            ModelApi::Bedrock,
            "https://bedrock-runtime.us-east-1.amazonaws.com",
        ),
        (
            Preset::Vertex,
            ModelApi::Vertex,
            "https://aiplatform.googleapis.com",
        ),
        (
            Preset::VertexAdc,
            ModelApi::Vertex,
            "https://aiplatform.googleapis.com",
        ),
    ]
    .into_iter()
    .chain(super::super::super::vendors::Vendor::ALL.map(|vendor| {
        (
            Preset::Hosted(vendor),
            Preset::Hosted(vendor).api(),
            vendor.endpoint(),
        )
    })) {
        for remote in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let node = runtime
                .block_on(Node::start(directory.path().join("node")))
                .unwrap();
            let other = runtime
                .block_on(Node::start(directory.path().join("other")))
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
                let workspace = cx.new(|cx| Workspace::new(window, cx));
                workspace.update(cx, |workspace, cx| {
                    workspace.bind_providers(
                        transport.clone(),
                        runtime.clone(),
                        "Fixture Node".into(),
                        cx,
                    )
                });
                owner = Some(workspace.clone());
                Root::new(workspace, window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| {
                owner.read(cx).provider_link.as_ref().unwrap().connected
            });
            assert!(owner.read_with(visual, |owner, _| owner.providers.channels.is_empty()));
            let editor = visual.update(|window, cx| {
                let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
                editor.update(cx, |editor, cx| {
                    editor.select_preset(preset, window, cx);
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Fixture provider", window, cx)
                    });
                    if preset.custom_endpoint() {
                        editor
                            .endpoint
                            .update(cx, |input, cx| input.set_value(endpoint, window, cx));
                    }
                    editor.cloud.project.update(cx, |input, cx| {
                        input.set_value("fixture-project", window, cx)
                    });
                    if preset.key_auth() {
                        editor.credential.update(cx, |input, cx| {
                            input.set_value("synthetic-editor-secret", window, cx)
                        });
                    }
                    editor.add_model(window, cx);
                    editor.models[0]
                        .id
                        .update(cx, |input, cx| input.set_value("fixture-model", window, cx));
                });
                // An already open editor must not follow a later host selection.
                owner.update(cx, |owner, cx| {
                    owner.bind_providers(other.local(), runtime.clone(), "Other Node".into(), cx)
                });
                editor.update(cx, |editor, cx| editor.save(window, cx));
                editor
            });
            wait(visual, |cx| !editor.read(cx).pending);
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert!(editor.credential.read(cx).value().is_empty());
            });
            let Output::Snapshot(snapshot) = runtime
                .block_on(client.execute(client.prepare(Command::Snapshot)))
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert_eq!(snapshot.providers.len(), 1);
            let provider = snapshot.providers[0].clone();
            assert_eq!(provider.api, api);
            assert_eq!(provider.endpoint, endpoint);
            assert_eq!(provider.default_model, "fixture-model");
            assert_eq!(provider.credential.is_some(), preset.key_auth());
            assert_eq!(provider.authentication, preset.authentication());
            assert_eq!(
                provider.options,
                editor.read_with(visual, |editor, cx| editor
                    .cloud
                    .options(preset, cx)
                    .unwrap())
            );
            let other_client = Client::new(other.local());
            let Output::Snapshot(snapshot) = runtime
                .block_on(other_client.execute(other_client.prepare(Command::Snapshot)))
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert!(snapshot.providers.is_empty());
            visual.update(|_, cx| {
                owner.update(cx, |owner, cx| {
                    owner.bind_providers(
                        transport.clone(),
                        runtime.clone(),
                        "Fixture Node".into(),
                        cx,
                    )
                })
            });
            wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
            let id = owner.read_with(visual, |owner, _| owner.providers.channels[0].id);
            owner.read_with(visual, |owner, _| {
                assert_eq!(owner.providers.channels[0].preset, preset)
            });
            let editor = visual
                .update(|window, cx| cx.new(|cx| Editor::new(owner.clone(), Some(id), window, cx)));
            wait(visual, |cx| !editor.read(cx).pending);
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert!(!editor.credential.read(cx).presentation().is_masked());
                assert_eq!(
                    editor.credential.read(cx).presentation().placeholder(),
                    &tr("provider_key_hint")
                );
                assert_eq!(
                    editor.credential.read(cx).value().as_ref(),
                    if preset.key_auth() {
                        "synthetic-editor-secret"
                    } else {
                        ""
                    }
                );
            });
            let mut changed = provider.clone();
            changed.name = "Changed by another client".into();
            runtime
                .block_on(client.execute(client.prepare(Command::PutProvider {
                    provider: changed,
                    expected_revision: provider.revision,
                })))
                .unwrap();
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Preserved draft", window, cx)
                    });
                    if preset.key_auth() {
                        editor.credential.update(cx, |input, cx| {
                            input.set_value("synthetic-new-secret", window, cx)
                        });
                    }
                    editor.save(window, cx);
                })
            });
            wait(visual, |cx| !editor.read(cx).pending);
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, Some("provider_revision_changed"));
                assert_eq!(editor.name.read(cx).value().as_ref(), "Preserved draft");
                assert_eq!(
                    editor.credential.read(cx).value().as_ref(),
                    if !preset.key_auth() {
                        ""
                    } else {
                        "synthetic-new-secret"
                    }
                );
            });
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].name == "Changed by another client"
            });
            visual.update(|_, cx| {
                owner.update(cx, |owner, cx| owner.provider_action(id, false, false, cx))
            });
            wait(visual, |cx| {
                !owner.read(cx).provider_link.as_ref().unwrap().pending
                    && !owner.read(cx).providers.channels[0].enabled
            });
            visual.update(|_, cx| {
                owner.update(cx, |owner, cx| owner.provider_action(id, true, false, cx))
            });
            wait(visual, |cx| owner.read(cx).providers.channels.is_empty());
            visual.update(|window, _| window.remove_window());
            runtime.block_on(node.shutdown()).unwrap();
            runtime.block_on(other.shutdown()).unwrap();
            runtime.block_on(controller.close()).unwrap();
        }
    }
}
