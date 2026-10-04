use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider, discovery},
    *,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

mod discovery_support;
use discovery_support::{Reply, Server};
#[path = "discovery/anthropic.rs"]
mod anthropic;
#[path = "discovery/endpoints.rs"]
mod endpoints;
#[path = "discovery/lifecycle.rs"]
mod lifecycle;
#[path = "discovery/live.rs"]
mod live;
#[path = "discovery/responses.rs"]
mod responses;

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Arc<Client>,
}

impl Fixture {
    async fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
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
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        Self {
            _directory: directory,
            node,
            controller,
            client,
        }
    }
    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

const KEY: &str = "isolated-discovery-key";

fn source(api: ModelApi, server: &Server) -> discovery::Draft {
    discovery::Draft {
        provider: ProviderId::new(),
        api,
        endpoint: server.endpoint.clone(),
        credential: None,
        secret: Some(Secret::new(KEY.into())),
    }
}

fn catalog(api: ModelApi, second: bool) -> Value {
    let known = if api == ModelApi::Gemini {
        json!({"name":"models/known", "inputTokenLimit":4096, "outputTokenLimit":64, "supportedGenerationMethods":["generateContent"]})
    } else if api == ModelApi::Anthropic {
        json!({"id":"known", "max_input_tokens":4096, "max_tokens":64})
    } else {
        json!({"id":"known", "context_length":4096, "max_output_tokens":64})
    };
    let unknown = if api == ModelApi::Gemini {
        json!({"name":"models/unknown", "supportedGenerationMethods":["generateContent"]})
    } else {
        json!({"id":"unknown"})
    };
    match api {
        ModelApi::Gemini if second => json!({"models":[known, unknown]}),
        ModelApi::Gemini => {
            json!({"models":[known, {"name":"models/embedding", "supportedGenerationMethods":["embedContent"]}], "nextPageToken":"next+/="})
        }
        ModelApi::Anthropic if second => json!({"data":[known, unknown], "has_more":false}),
        ModelApi::Anthropic => json!({"data":[known], "has_more":true, "last_id":"known"}),
        _ => json!({"data":[known.clone(), unknown, known]}),
    }
}

fn discover(draft: discovery::Draft) -> Command {
    Command::DiscoverModels(Box::new(discovery::Source::Draft(draft)))
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[tokio::test]
async fn discovers_without_persistence() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for api in [
            ModelApi::ChatCompletions,
            ModelApi::Responses,
            ModelApi::Anthropic,
            ModelApi::Gemini,
        ] {
            let server = Server::start(api, move |path| {
                Reply::Json(catalog(
                    api,
                    path.contains("after_id=") || path.contains("pageToken="),
                ))
            })
            .await;
            let mut input = source(api, &server);
            let request = fixture.client.prepare(discover(input.clone()));
            assert!(!format!("{request:?}").contains(KEY));
            let admission = fixture.client.dispatch(request).await.unwrap();
            assert!(!admission.receipt.durable);
            let Output::DiscoveredModels(discovery::Catalog { models, .. }) =
                admission.completion.await.unwrap().unwrap()
            else {
                panic!("models expected")
            };
            assert_eq!(
                models,
                vec![
                    discovery::Model {
                        id: "known".into(),
                        context: Some(4096),
                        output: Some(64),
                        capabilities: None,
                    },
                    discovery::Model {
                        id: "unknown".into(),
                        context: None,
                        output: None,
                        capabilities: None,
                    }
                ]
            );
            let provider = Provider {
                options: None,
                id: input.provider,
                revision: 0,
                name: "Discovery fixture".into(),
                api,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: server.endpoint.clone(),
                enabled: true,
                credential: None,
                default_model: "known".into(),
                models: ["known", "unknown", "missing"]
                    .into_iter()
                    .map(|id| Model {
                        id: id.into(),
                        context: 256,
                        output: 128,
                        vision: false,
                        tools: false,
                        reasoning: false,
                        web_search: false,
                        generates: vec![],
                        efforts: vec![],
                        custom_efforts: false,
                        default_effort: sailry_protocol::Effort::Default,
                    })
                    .collect(),
            };
            let Output::Provider(provider) = execute(
                &fixture.client,
                Command::SaveProvider {
                    provider,
                    expected_revision: 0,
                    secret: input.secret.take(),
                },
            )
            .await
            else {
                panic!("provider expected")
            };
            input.credential = provider.credential.clone();
            let db =
                rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                    .unwrap();
            let before: i64 = db
                .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
                .unwrap();
            assert_eq!(
                execute(&fixture.client, discover(input)).await,
                Output::DiscoveredModels(discovery::Catalog {
                    endpoint: server.endpoint.clone(),
                    models
                })
            );
            let Output::ProviderValidation(result) = execute(
                &fixture.client,
                Command::ValidateProvider {
                    provider: provider.id,
                    expected_revision: provider.revision,
                },
            )
            .await
            else {
                panic!("validation expected")
            };
            assert_eq!(result.missing, ["missing"]);
            assert_eq!(result.exceeded, ["known"]);
            assert_eq!(result.unverified, ["unknown"]);
            assert_eq!(
                before,
                db.query_row::<i64, _, _>("SELECT count(*) FROM requests", [], |row| row.get(0))
                    .unwrap()
            );
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests.len(),
                if matches!(api, ModelApi::Anthropic | ModelApi::Gemini) {
                    6
                } else {
                    3
                }
            );
            for request in requests.iter() {
                let header = match api {
                    ModelApi::Anthropic => "x-api-key",
                    ModelApi::Gemini => "x-goog-api-key",
                    _ => "authorization",
                };
                assert_eq!(
                    request.headers[header],
                    if header == "authorization" {
                        format!("Bearer {KEY}")
                    } else {
                        KEY.into()
                    }
                );
                assert!(!request.path.contains(KEY));
                assert!(request.path.starts_with(if api == ModelApi::Anthropic {
                    "/v1/models?"
                } else if api == ModelApi::Gemini {
                    "/v1beta/models?"
                } else {
                    "/v1/models"
                }));
            }
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn releases_cancelled_queries() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let server = Server::start(ModelApi::Responses, |_| Reply::Hold).await;
        let mut jobs = Vec::new();
        for _ in 0..4 {
            let client = fixture.client.clone();
            let command = discover(source(ModelApi::Responses, &server));
            jobs.push(tokio::spawn(async move {
                client.execute(client.prepare(command)).await
            }));
        }
        server.wait_requests(4).await;
        let result = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(discover(source(ModelApi::Responses, &server))),
            )
            .await;
        assert_eq!(result.unwrap_err().code, ErrorCode::Busy);
        assert!(matches!(
            execute(&fixture.client, Command::Snapshot).await,
            Output::Snapshot(_)
        ));
        for job in jobs {
            job.abort();
            let _ = job.await;
        }
        server.wait_closed(4).await;
        let available =
            Server::start(ModelApi::Responses, |_| Reply::Json(json!({"data":[]}))).await;
        assert_eq!(
            execute(
                &fixture.client,
                discover(source(ModelApi::Responses, &available))
            )
            .await,
            Output::DiscoveredModels(discovery::Catalog {
                endpoint: available.endpoint.clone(),
                models: vec![]
            })
        );
        fixture.close().await;
    }
}
