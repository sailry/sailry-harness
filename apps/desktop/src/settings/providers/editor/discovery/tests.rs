use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::Secret;
use sailry_protocol::conversation::{Model, ModelApi, Provider};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use super::super::discovery_server::{Reply, Server};

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "discovery update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Fixture {
    runtime: Arc<tokio::runtime::Runtime>,
    node: Node,
    controller: Link,
    transport: Arc<dyn Transport>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new(remote: bool) -> Self {
        Self::with_catalog(remote, None)
    }
    fn with_catalog(remote: bool, endpoint: Option<&str>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(async {
                if let Some(endpoint) = endpoint {
                    Node::start_with_catalog(directory.path().join("node"), endpoint).await
                } else {
                    Node::start(directory.path().join("node")).await
                }
            })
            .unwrap();
        Self::with_node(remote, runtime, directory, node)
    }
    fn with_node(
        remote: bool,
        runtime: Arc<tokio::runtime::Runtime>,
        directory: tempfile::TempDir,
        node: Node,
    ) -> Self {
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        Self {
            runtime,
            node,
            controller,
            transport,
            _directory: directory,
        }
    }
    fn owner(&self, window: &mut Window, cx: &mut App) -> Entity<Workspace> {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        owner.update(cx, |owner, cx| {
            owner.bind_providers(
                self.transport.clone(),
                self.runtime.clone(),
                "Discovery Node".into(),
                cx,
            )
        });
        owner
    }
    fn execute(&self, command: Command) -> Output {
        let client = Client::new(self.transport.clone());
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }
    fn close(self) {
        self.runtime.block_on(self.controller.close()).unwrap();
        self.runtime.block_on(self.node.shutdown()).unwrap();
    }
}

#[path = "tests/authorization.rs"]
mod authorization;
#[path = "tests/catalog.rs"]
mod catalog;
#[path = "tests/completion.rs"]
mod completion;
#[path = "tests/reasoning.rs"]
mod reasoning;

#[gpui::test]
fn merges_without_overwriting_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let server = fixture
            .runtime
            .block_on(Server::start(ModelApi::Responses, move |_| {
                Reply::Delayed(
                    gate.clone(),
                    json!({"data":[
                        {"id":"manual", "context_length":2048, "max_output_tokens":64},
                        {"id":"new", "context_length":4096, "max_output_tokens":128},
                        {"id":"unknown"},
                        {"id":"saved", "context_length":4096, "max_output_tokens":128}
                    ]}),
                )
            }));
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = fixture.owner(window, cx);
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            view.update(cx, |editor, cx| {
                editor.select_preset(Preset::CompatibleResponses, window, cx);
                editor.endpoint.update(cx, |input, cx| {
                    input.set_value(&server.endpoint, window, cx)
                });
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("manual", window, cx));
                let manual = super::super::super::data::Model {
                    context: 32768,
                    ..super::super::super::data::Model::example("saved")
                };
                editor.models.push(Draft::new(1, manual, window, cx));
                editor.next_model = 2;
                editor.discover(window, cx);
            });
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        fixture.runtime.block_on(server.wait_requests(1));
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.name.update(cx, |input, cx| {
                    input.set_value("Edited while loading", window, cx)
                });
                editor.add_model(window, cx);
                editor.models[2]
                    .id
                    .update(cx, |input, cx| input.set_value("another", window, cx));
                editor.default_model = 2;
            })
        });
        release.notify_one();
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.models.len(), 5);
            assert_eq!(editor.default_model, 2);
            assert_eq!(editor.models[0].value(cx).unwrap().context, 2048);
            assert_eq!(editor.models[1].value(cx).unwrap().context, 32768);
            assert_eq!(editor.models[3].value(cx).unwrap().context, 4096);
            assert!(!editor.models[3].value(cx).unwrap().tools);
            assert_eq!(editor.models[4].value(cx).unwrap().context, 200000);
            assert_eq!(
                editor.name.read(cx).value().as_ref(),
                "Edited while loading"
            );
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
fn cancels_stale_connections(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let delayed = fixture
            .runtime
            .block_on(Server::start(ModelApi::Responses, move |_| {
                Reply::Delayed(gate.clone(), json!({"data":[{"id":"late"}]}))
            }));
        let held = fixture
            .runtime
            .block_on(Server::start(ModelApi::Responses, |_| Reply::Hold));
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = fixture.owner(window, cx);
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            view.update(cx, |editor, cx| {
                editor.select_preset(Preset::CompatibleResponses, window, cx);
                editor.endpoint.update(cx, |input, cx| {
                    input.set_value(&delayed.endpoint, window, cx)
                });
                editor.discover(window, cx);
            });
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        fixture.runtime.block_on(delayed.wait_requests(1));
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .endpoint
                    .update(cx, |input, cx| input.set_value(&held.endpoint, window, cx))
            })
        });
        release.notify_one();
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert!(editor.models.is_empty());
            assert_eq!(editor.error, Some("provider_query_changed"));
            assert_eq!(editor.endpoint.read(cx).value().as_ref(), held.endpoint);
        });
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.discover(window, cx)));
        fixture.runtime.block_on(held.wait_requests(1));
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.close(window, cx)));
        fixture.runtime.block_on(held.wait_closed(1));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn refreshes_saved_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(Server::start(ModelApi::Gemini, |_| Reply::Json(json!({"models":[
            {"name":"models/known", "inputTokenLimit":1024, "outputTokenLimit":64, "supportedGenerationMethods":["generateContent"]}
        ]}))));
        let Output::Provider(provider) = fixture.execute(Command::SaveProvider {
            provider: Provider {
                options: None,
                id: sailry_protocol::ProviderId::new(),
                revision: 0,
                name: "Saved".into(),
                api: ModelApi::Gemini,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: server.endpoint.clone(),
                enabled: true,
                credential: None,
                default_model: "known".into(),
                models: vec![Model {
                    id: "known".into(),
                    context: 1024,
                    output: 64,
                    vision: false,
                    tools: false,
                    reasoning: false,
                    web_search: false,
                    generates: vec![],
                    efforts: vec![],
                    custom_efforts: false,
                    default_effort: sailry_protocol::Effort::Default,
                }],
            },
            expected_revision: 0,
            secret: Some(Secret::new("synthetic-discovery-key".into())),
        }) else {
            panic!("provider expected")
        };
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = fixture.owner(window, cx);
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
        let editor = visual.update(|window, cx| {
            let editor = cx.new(|cx| Editor::new(owner.clone(), Some(0), window, cx));
            editor.update(cx, |editor, cx| {
                editor
                    .name
                    .update(cx, |input, cx| input.set_value("Unsaved name", window, cx));
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("unsaved-model", window, cx));
            });
            editor
        });
        wait(visual, |cx| !editor.read(cx).pending);
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.discover(window, cx)));
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.error, None);
            assert_eq!(
                editor.credential.read(cx).value(),
                "synthetic-discovery-key"
            );
            assert_eq!(editor.name.read(cx).value().as_ref(), "Unsaved name");
            assert_eq!(
                editor.models[0].id.read(cx).value().as_ref(),
                "unsaved-model"
            );
            assert_eq!(editor.models.len(), 2);
            let discovered = editor.models[1].value(cx).unwrap();
            assert_eq!(discovered.id, "known");
            assert_eq!((discovered.context, discovered.output), (1024, 64));
        });
        assert_eq!(
            server.requests.lock().unwrap()[0].headers["x-goog-api-key"],
            "synthetic-discovery-key"
        );
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .credential
                    .update(cx, |input, cx| input.set_value("unsaved-key", window, cx));
                editor.discover(window, cx);
            });
        });
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.error, None);
            assert_eq!(editor.name.read(cx).value().as_ref(), "Unsaved name");
            assert_eq!(editor.credential.read(cx).value().as_ref(), "unsaved-key");
            assert_eq!(editor.models.len(), 2);
            assert_eq!(editor.default_model, 0);
        });
        assert_eq!(
            server.requests.lock().unwrap()[1].headers["x-goog-api-key"],
            "unsaved-key"
        );
        let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        assert_eq!(providers, vec![provider]);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
