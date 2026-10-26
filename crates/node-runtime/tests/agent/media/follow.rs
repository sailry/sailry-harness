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
        Self::with_outcome(remote, api, wire::Outcome::Completed).await
    }

    async fn with_outcome(remote: bool, api: ModelApi, outcome: wire::Outcome) -> Self {
        let server = wire::Server::with_outcome(outcome).await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_model_transport_fixture(adk_model::openai::WsOptions {
            enabled: std::sync::Arc::new(|| false),
            ..Default::default()
        });
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
        let package = install(&client).await;
        let (base, mut provider) = configured_session(&client, &server.endpoint, &root).await;
        provider.api = api;
        provider.options = None;
        provider.models = ["follow-a", "follow-b"]
            .into_iter()
            .map(|id| {
                let mut value = model(id);
                value.vision = false;
                value.tools = false;
                value.generates.clear();
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

    async fn companion(&self) -> (wire::Server, Provider) {
        let server = wire::Server::start().await;
        let mut provider = self.provider.clone();
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.credential = None;
        provider.endpoint = server.endpoint.clone();
        provider.api = ModelApi::ChatCompletions;
        provider.options = None;
        let mut metadata = model("override-image");
        metadata.generates.clear();
        provider.models = vec![metadata];
        provider.default_model = "override-image".into();
        let Output::Provider(provider) = self
            .execute(Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new("isolated-override-image-key".into())),
            })
            .await
        else {
            panic!("provider expected")
        };
        self.execute(Command::SaveMediaSettings(Settings {
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
        (server, provider)
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
        .any(|tool| {
            tool["name"] == plugin_tool("media", "generate_image")
                || tool["function"]["name"] == plugin_tool("media", "generate_image")
        })
}

fn assert_route(request: &wire::Request, api: ModelApi, model: &str) {
    if api == ModelApi::Gemini {
        assert_eq!(request.path, format!("/v1/models/{model}:generateContent"));
        assert_eq!(request.headers["x-goog-api-key"], wire::KEY);
        assert_eq!(
            request.body["generationConfig"]["responseModalities"],
            json!(["TEXT", "IMAGE"])
        );
    } else if api == ModelApi::Responses {
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
    } else {
        assert_eq!(request.path, "/v1/images/generations");
        assert_eq!(request.body["model"], model);
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {}", wire::KEY)
        );
    }
}

#[tokio::test]
async fn resolves_admitted_and_future_models() {
    for remote in [false, true] {
        for api in [
            ModelApi::Responses,
            ModelApi::Gemini,
            ModelApi::ChatCompletions,
        ] {
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
async fn does_not_repeat_same_fallback() {
    for remote in [false, true] {
        let fixture = Fixture::with_outcome(
            remote,
            ModelApi::Responses,
            wire::Outcome::Rejected(400, "unsupported_capability"),
        )
        .await;
        fixture
            .execute(Command::SaveMediaSettings(Settings {
                revision: 0,
                bindings: [(
                    Kind::Image,
                    Some(Binding {
                        provider: fixture.provider.id,
                        model: "follow-a".into(),
                    }),
                )]
                .into(),
            }))
            .await;
        let session = fixture.create().await;
        let turn = fixture.queue(&session).await;
        fixture.begin(&turn).await;
        let path = "assets/generated/duplicate.png";
        assert_eq!(
            fixture.image(&session, &turn, path).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert!(!fixture.root.join(path).exists());
        fixture.stop(&turn).await;
        {
            let requests = fixture.server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                requests.iter().filter(|request| !request.primary()).count(),
                1
            );
            assert_route(
                requests.iter().find(|request| !request.primary()).unwrap(),
                ModelApi::Responses,
                "follow-a",
            );
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn failures_never_use_companion() {
    for remote in [false, true] {
        for api in [ModelApi::Responses, ModelApi::Gemini] {
            let fixture = Fixture::new(remote, api).await;
            let mut provider = fixture.provider.clone();
            provider.request_max_retries = Some(0);
            provider.stream_max_retries = Some(0);
            fixture
                .execute(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                })
                .await;
            let (companion, _) = fixture.companion().await;
            let session = fixture.create().await;
            let turn = fixture.queue(&session).await;
            fixture.begin(&turn).await;
            let outcomes = [
                wire::Outcome::Rejected(401, "invalid_api_key"),
                wire::Outcome::Rejected(403, "access_denied"),
                wire::Outcome::Rejected(429, "rate_limit_exceeded"),
                wire::Outcome::Rejected(500, "internal_error"),
                wire::Outcome::Rejected(503, "service_unavailable"),
                wire::Outcome::Rejected(400, "invalid_request"),
                wire::Outcome::Rejected(404, "missing_resource"),
                wire::Outcome::Disconnected,
                wire::Outcome::Invalid,
                wire::Outcome::Empty,
                wire::Outcome::Incomplete,
                wire::Outcome::Rejected(401, "unsupported_capability"),
                wire::Outcome::Rejected(403, "unsupported_capability"),
                wire::Outcome::Rejected(429, "unsupported_capability"),
            ];
            for (index, outcome) in outcomes.into_iter().enumerate() {
                fixture.server.set_outcome(outcome);
                let path = format!("assets/generated/failure-{index}.png");
                let error = fixture.image(&session, &turn, &path).await.unwrap_err();
                assert!(
                    matches!(
                        error.code,
                        ErrorCode::Unavailable | ErrorCode::OutcomeUnknown
                    ),
                    "{remote:?} {outcome:?}: {error:?}"
                );
                assert!(!fixture.root.join(&path).exists(), "{outcome:?}");
                assert!(companion.requests.lock().unwrap().is_empty(), "{outcome:?}");
                let requests = fixture.server.requests.lock().unwrap();
                let generated: Vec<_> = requests
                    .iter()
                    .filter(|request| !request.primary())
                    .collect();
                assert_eq!(
                    generated.len(),
                    index + 1,
                    "failure must not replay: {outcome:?}"
                );
                assert_route(generated.last().unwrap(), api, "follow-a");
            }
            fixture.stop(&turn).await;
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn uses_companion_only_when_unsupported() {
    for remote in [false, true] {
        for api in [
            ModelApi::Responses,
            ModelApi::Gemini,
            ModelApi::ChatCompletions,
            ModelApi::Anthropic,
        ] {
            let fixture = Fixture::new(remote, api).await;
            let (companion, _) = fixture.companion().await;
            let session = fixture.create().await;
            fixture
                .execute(Command::SaveMediaSettings(Settings {
                    revision: 1,
                    bindings: Default::default(),
                }))
                .await;
            let without_companion = fixture.create().await;
            let unsupported = api == ModelApi::Anthropic;
            let mut outcomes = vec![wire::Outcome::Completed];
            if !unsupported {
                outcomes.push(wire::Outcome::Rejected(400, "unsupported_capability"));
                if api == ModelApi::Responses {
                    outcomes.extend([
                        wire::Outcome::Rejected(422, "unsupported_tool"),
                        wire::Outcome::Rejected(405, "route_unavailable"),
                        wire::Outcome::Rejected(501, "route_unavailable"),
                    ]);
                }
            }
            let mut fallback_count = 0;
            for (index, outcome) in outcomes.into_iter().enumerate() {
                fixture.server.set_outcome(outcome);
                let turn = fixture.queue(&session).await;
                fixture.begin(&turn).await;
                fixture
                    .generated(
                        &session,
                        &turn,
                        &format!("assets/generated/selected-{index}.png"),
                    )
                    .await;
                fixture.stop(&turn).await;
                let fallback = unsupported || !matches!(outcome, wire::Outcome::Completed);
                fallback_count += usize::from(fallback);
                {
                    let requests = fixture.server.requests.lock().unwrap();
                    let generated: Vec<_> = requests
                        .iter()
                        .filter(|request| !request.primary())
                        .collect();
                    assert_eq!(
                        generated.len(),
                        if unsupported { 0 } else { index + 1 },
                        "{api:?} {outcome:?}"
                    );
                    if !unsupported {
                        assert_route(generated.last().unwrap(), api, "follow-a");
                    }
                    let images = companion.requests.lock().unwrap();
                    assert_eq!(images.len(), fallback_count, "{api:?} {outcome:?}");
                    for request in images.iter() {
                        assert_eq!(request.path, "/v1/images/generations");
                        assert_eq!(request.body["model"], "override-image");
                        assert_eq!(
                            request.headers["authorization"],
                            "Bearer isolated-override-image-key"
                        );
                    }
                }
            }
            let before = fixture.server.requests.lock().unwrap().len();
            let turn = fixture.queue(&without_companion).await;
            fixture.begin(&turn).await;
            let error = fixture
                .image(
                    &without_companion,
                    &turn,
                    "assets/generated/unconfigured.png",
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::NotConfigured, "{api:?}: {error:?}");
            assert!(
                !fixture
                    .root
                    .join("assets/generated/unconfigured.png")
                    .exists()
            );
            fixture.stop(&turn).await;
            assert_eq!(
                fixture.server.requests.lock().unwrap().len(),
                before + if unsupported { 1 } else { 2 },
                "{api:?}"
            );
            assert_eq!(
                companion.requests.lock().unwrap().len(),
                fallback_count,
                "{api:?}"
            );
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn retains_frozen_companion_after_revisions() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, ModelApi::Anthropic).await;
        let (companion, mut provider) = fixture.companion().await;
        let session = fixture.create().await;
        let sent = fixture.queue(&session).await;
        let changed = fixture.revise(&session).await;
        provider.endpoint = "http://127.0.0.1:9/revised-image".into();
        provider.models[0].id = "revised-image".into();
        provider.default_model = "revised-image".into();
        fixture
            .execute(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new("isolated-revised-image-key".into())),
            })
            .await;
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
