use super::*;
use sailry_protocol::media::Action;
use std::path::PathBuf;

#[path = "follow/server.rs"]
mod wire;

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
    provider: Provider,
    package: plugin::Info,
    server: wire::Server,
}

impl Fixture {
    async fn new(remote: bool, api: ModelApi) -> Self {
        let server = wire::Server::start().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let (base, mut provider) = configured(&client, &server.endpoint, &root).await;
        provider.api = api;
        provider.models = ["follow-a", "follow-b"]
            .into_iter()
            .map(|id| {
                let mut value = model(id);
                value.generates = vec![if api == ModelApi::Gemini {
                    Generation::Image
                } else {
                    Generation::ImageTool
                }];
                value
            })
            .collect();
        provider.default_model = "follow-a".into();
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new(wire::KEY.into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: base.id,
                expected_revision: base.revision,
                config: SessionConfig {
                    model: "follow-a".into(),
                    permission: Permission::Full,
                    effort: Effort::Default,
                    credential: provider.credential.clone(),
                    ..base.config
                },
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "media".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("media package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert!(package.summary.enabled);
        assert!(package.skills.iter().any(|skill| skill.name == "imagegen"));
        Self {
            _directory: directory,
            root,
            node,
            controller,
            client,
            session,
            provider,
            package,
            server,
        }
    }

    async fn execute(&self, command: Command) -> Output {
        self.client
            .execute(self.client.prepare(command))
            .await
            .unwrap()
    }

    async fn create(&self) -> Session {
        let Output::Session(session) = self
            .execute(Command::CreateSession {
                project: self.session.project,
                worktree: Some(self.session.worktree),
                config: Some(self.session.config.clone()),
            })
            .await
        else {
            panic!("session expected")
        };
        session
    }

    async fn revise(&self, session: &Session) -> Session {
        let Output::Session(session) = self
            .execute(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: SessionConfig {
                    model: "follow-b".into(),
                    ..session.config.clone()
                },
            })
            .await
        else {
            panic!("session expected")
        };
        session
    }

    async fn queue(&self, session: &Session) -> QueuedTurn {
        let Output::QueuedTurn(turn) = self
            .execute(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Draw a pixel".into(),
            })
            .await
        else {
            panic!("turn expected")
        };
        assert!(turn.plugins.contains(&self.package.summary.reference()));
        turn
    }

    async fn begin(&self, turn: &QueuedTurn) {
        let Output::QueuedMessage(queued) = self
            .execute(Command::ReadQueuedTurn { turn: turn.id })
            .await
        else {
            panic!("queued message expected")
        };
        assert_eq!(queued.turn, *turn);
        let page = history(&self.client, turn.session).await;
        if page.queue.paused {
            self.execute(Command::SetQueuePaused {
                session: turn.session,
                expected_revision: page.queue.revision,
                paused: false,
            })
            .await;
        }
        let count = self
            .server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.primary())
            .count();
        self.execute(Command::StartQueuedTurn { turn: turn.id })
            .await;
        if self.server.wait_primary(count + 1).await.is_err() {
            let page = history(&self.client, turn.session).await;
            let requests: Vec<_> = self
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| {
                    json!({"path":request.path,"model":request.body["model"],
                        "stream":request.body["stream"],"primary":request.primary()})
                })
                .collect();
            panic!(
                "admitted model request deadline: expected={}, turn={turn:?}, api={:?}, endpoint={}, requests={requests:?}, history={page:?}",
                count + 1,
                self.provider.api,
                self.provider.endpoint
            );
        }
    }

    async fn image(
        &self,
        session: &Session,
        turn: &QueuedTurn,
        path: &str,
    ) -> Result<Output, Fault> {
        self.client
            .execute(
                self.client
                    .prepare(Command::UseMedia {
                        turn: turn.id,
                        session: session.id,
                        worktree: session.worktree,
                        action: Action::Image {
                            prompt: "A pixel".into(),
                            path: path.into(),
                        },
                    })
                    .with_plugin(plugin::Context {
                        invocation: None,
                        turn: Some(turn.id),
                        surface: Default::default(),
                        package: self.package.summary.reference(),
                        worktree: Some(session.worktree),
                        session: Some(session.id),
                    }),
            )
            .await
    }

    async fn generated(&self, session: &Session, turn: &QueuedTurn, path: &str) {
        let output = self.image(session, turn, path).await.unwrap();
        assert!(matches!(output, Output::Media(value) if value["path"] == path));
        assert_eq!(
            std::fs::read(self.root.join(path)).unwrap(),
            server::image()
        );
    }

    async fn stop(&self, turn: &QueuedTurn) {
        self.execute(Command::StopTurn { turn: turn.id }).await;
        let page = finished(&self.client, turn.session, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(!serde_json::to_string(&page).unwrap().contains(wire::KEY));
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn registered(request: &wire::Request) -> bool {
    request.body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == plugin_tool("media", "generate_image"))
}

fn assert_route(request: &wire::Request, api: ModelApi, model: &str) {
    if api == ModelApi::Gemini {
        assert_eq!(request.path, format!("/v1/models/{model}:generateContent"));
        assert_eq!(request.headers["x-goog-api-key"], wire::KEY);
        assert_eq!(
            request.body["generationConfig"]["responseModalities"],
            json!(["TEXT", "IMAGE"])
        );
    } else {
        assert_eq!(request.path, "/v1/responses");
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {}", wire::KEY)
        );
        assert_eq!(request.body["model"], model);
        assert_eq!(request.body["stream"], false);
        assert_eq!(
            request.body["tools"],
            json!([{"type":"image_generation", "output_format":"png"}])
        );
        assert_eq!(
            request.body["tool_choice"],
            json!({"type":"image_generation"})
        );
    }
}

#[tokio::test]
async fn resolves_admitted_and_future_models() {
    for remote in [false, true] {
        for api in [ModelApi::Responses, ModelApi::Gemini] {
            let fixture = Fixture::new(remote, api).await;
            let Output::MediaSettings(settings) = fixture.execute(Command::ReadMediaSettings).await
            else {
                panic!("settings expected")
            };
            assert!(settings.bindings.is_empty());
            let sent = fixture.queue(&fixture.session).await;
            let changed = fixture.revise(&fixture.session).await;
            assert_eq!(sent.config.model, "follow-a");
            assert_eq!(changed.config.model, "follow-b");
            fixture.begin(&sent).await;
            fixture
                .generated(&changed, &sent, "assets/generated/sent.png")
                .await;
            fixture.stop(&sent).await;
            let future = fixture.queue(&changed).await;
            assert_eq!(future.config.model, "follow-b");
            fixture.begin(&future).await;
            fixture
                .generated(&changed, &future, "assets/generated/future.png")
                .await;
            fixture.stop(&future).await;
            {
                let requests = fixture.server.requests.lock().unwrap();
                let primary: Vec<_> = requests
                    .iter()
                    .filter(|request| request.primary())
                    .collect();
                let generated: Vec<_> = requests
                    .iter()
                    .filter(|request| !request.primary())
                    .collect();
                assert_eq!(primary.len(), 2);
                assert_eq!(generated.len(), 2);
                for (index, model) in ["follow-a", "follow-b"].into_iter().enumerate() {
                    if api == ModelApi::Gemini {
                        assert_eq!(
                            primary[index].path,
                            format!("/v1/models/{model}:streamGenerateContent?alt=sse")
                        );
                        assert_eq!(
                            primary[index].headers["x-goog-api-key"],
                            generated[index].headers["x-goog-api-key"]
                        );
                    } else {
                        assert_eq!(primary[index].body["model"], model);
                        assert!(registered(primary[index]));
                        assert_eq!(
                            primary[index].headers["authorization"],
                            generated[index].headers["authorization"]
                        );
                    }
                    assert_route(generated[index], api, model);
                }
            }
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn disabled_role_has_no_generation_calls() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, ModelApi::Responses).await;
        fixture
            .execute(Command::SaveMediaSettings(Settings {
                revision: 0,
                bindings: [(Kind::Image, None)].into(),
            }))
            .await;
        let session = fixture.create().await;
        fixture
            .execute(Command::SaveMediaSettings(Settings {
                revision: 1,
                bindings: Default::default(),
            }))
            .await;
        let turn = fixture.queue(&session).await;
        fixture.begin(&turn).await;
        assert_eq!(
            fixture
                .image(&session, &turn, "assets/generated/disabled.png")
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(!fixture.root.join("assets/generated/disabled.png").exists());
        fixture.stop(&turn).await;
        {
            let requests = fixture.server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].primary());
            assert!(!registered(&requests[0]));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_frozen_companion_after_revisions() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, ModelApi::Responses).await;
        let companion = wire::Server::start().await;
        let mut provider = fixture.provider.clone();
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.credential = None;
        provider.endpoint = companion.endpoint.clone();
        provider.api = ModelApi::ChatCompletions;
        provider.models = vec![model("override-image")];
        provider.default_model = "override-image".into();
        let Output::Provider(provider) = fixture
            .execute(Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new("isolated-override-image-key".into())),
            })
            .await
        else {
            panic!("provider expected")
        };
        fixture
            .execute(Command::SaveMediaSettings(Settings {
                revision: 0,
                bindings: [(
                    Kind::Image,
                    Some(Binding {
                        provider: provider.id,
                        model: "override-image".into(),
                    }),
                )]
                .into(),
            }))
            .await;
        let session = fixture.create().await;
        let sent = fixture.queue(&session).await;
        let changed = fixture.revise(&session).await;
        fixture
            .execute(Command::SaveMediaSettings(Settings {
                revision: 1,
                bindings: Default::default(),
            }))
            .await;
        fixture.begin(&sent).await;
        fixture
            .generated(&changed, &sent, "assets/generated/override-sent.png")
            .await;
        fixture.stop(&sent).await;
        let future = fixture.queue(&changed).await;
        fixture.begin(&future).await;
        fixture
            .generated(&changed, &future, "assets/generated/override-future.png")
            .await;
        fixture.stop(&future).await;
        {
            let requests = fixture.server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests.iter().all(|request| request.primary()));
            assert_eq!(requests[0].body["model"], "follow-a");
            assert_eq!(requests[1].body["model"], "follow-b");
            let images = companion.requests.lock().unwrap();
            assert_eq!(images.len(), 2);
            for request in images.iter() {
                assert_eq!(request.path, "/v1/images/generations");
                assert_eq!(request.body["model"], "override-image");
                assert_eq!(
                    request.headers["authorization"],
                    "Bearer isolated-override-image-key"
                );
            }
        }
        fixture.close().await;
    }
}
