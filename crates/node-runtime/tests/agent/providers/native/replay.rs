//! Signed reasoning stays scoped to the persisted execution provider and model.
use super::*;
use serde_json::{Value, json};
use tracing_subscriber::layer::SubscriberExt;

const SIGNATURE: &str = "fixture-encrypted-reasoning";

fn signed(body: &Value) -> Vec<Value> {
    body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "reasoning")
        .cloned()
        .collect()
}

fn expected() -> Vec<Value> {
    vec![json!({
        "type":"reasoning",
        "summary":[{"type":"summary_text","text":"Read the file"}],
        "encrypted_content":SIGNATURE,
    })]
}

async fn fixture(server: &Server) -> Fixture {
    let fixture = Fixture::packages(ModelApi::Responses, &server.endpoint, &["files"]).await;
    std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
    fixture
}

async fn complete(fixture: &Fixture) -> Page {
    let turn = fixture.submit("Read native.txt").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
    assert!(!serde_json::to_string(&page).unwrap().contains(SIGNATURE));
    page
}

#[tokio::test]
async fn keeps_signed_items_through_tools() {
    let server = Server::start(ModelApi::Responses, Reply::Tool).await;
    let fixture = fixture(&server).await;
    let page = complete(&fixture).await;
    assert!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .any(|part| matches!(part, Part::ToolResult { .. }))
    );
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(signed(&requests[0].body).is_empty());
        assert_eq!(signed(&requests[1].body), expected());
        for request in requests.iter() {
            assert_eq!(
                request.body["include"],
                json!(["reasoning.encrypted_content"])
            );
            assert!(request.body.get("previous_response_id").is_none());
        }
        assert!(
            requests[1].body["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "function_call_output")
        );
    }
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn restores_signed_items_after_reopen() {
    let server = Server::start(ModelApi::Responses, Reply::Tool).await;
    let fixture = fixture(&server).await;
    let page = complete(&fixture).await;
    let before = {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        signed(&requests[1].body)
    };
    assert_eq!(before, expected());
    let Fixture {
        directory,
        node,
        controller,
        client,
        session,
        ..
    } = fixture;
    let profile = node.profile().to_owned();
    drop(client);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_empty(profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(controller.handle().remote(address));
    assert_eq!(history(&client, session.id).await, page);
    let Output::QueuedTurn(turn) = execute(
        &client,
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Continue from the persisted reply".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    let restored = finished(&client, session.id, turn.id).await;
    assert_eq!(
        restored.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        restored.runs
    );
    assert!(
        !serde_json::to_string(&restored)
            .unwrap()
            .contains(SIGNATURE)
    );
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(signed(&requests[2].body), before);
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn drops_signatures_when_owner_or_model_changes() {
    for replace_provider in [false, true] {
        let server = Server::start(ModelApi::Responses, Reply::Tool).await;
        let mut fixture = fixture(&server).await;
        complete(&fixture).await;
        assert_eq!(signed(&server.requests.lock().unwrap()[1].body), expected());
        let mut provider = fixture.provider.clone();
        let mut config = fixture.session.config.clone();
        let command = if replace_provider {
            // Same API, endpoint and model, but a different configured account owner.
            provider.id = ProviderId::new();
            provider.revision = 0;
            provider.credential = None;
            provider.name = "Other Responses account".into();
            Command::SaveProvider {
                expected_revision: 0,
                provider,
                secret: Some(Secret::new("other-provider-key".into())),
            }
        } else {
            let mut model = provider.models[0].clone();
            model.id = "fixture-b".into();
            config.model = model.id.clone();
            provider.models.push(model);
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }
        };
        let Output::Provider(provider) = execute(&fixture.client, command).await else {
            panic!("provider expected")
        };
        config.provider = provider.id;
        config.credential = provider.credential;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        complete(&fixture).await;
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(signed(&requests[2].body).is_empty());
            assert!(!requests[2].body.to_string().contains(SIGNATURE));
            assert_eq!(requests[2].body["model"], fixture.session.config.model);
            if replace_provider {
                assert_eq!(
                    requests[2].headers["authorization"],
                    "Bearer other-provider-key"
                );
            }
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

fn response(tool: bool, index: usize) -> server::Script {
    let mut bytes =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            .to_owned();
    for event in server::response_events(tool, false, index) {
        bytes.push_str(&format!("data: {event}\n\n"));
    }
    server::Script {
        bytes: bytes.into_bytes(),
        ending: server::Ending::Close,
    }
}

fn rejection() -> server::Script {
    let body = json!({"error":{"type":"invalid_request_error", "message":"The encrypted content could not be verified", "code":"invalid_encrypted_content"}}).to_string();
    server::Script {
        bytes: format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes(),
        ending: server::Ending::Close,
    }
}

#[tokio::test]
async fn removes_rejected_items_once() {
    for succeeds in [false, true] {
        let errors = Errors::default();
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(errors.clone()));
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![
                response(true, 0),
                rejection(),
                if succeeds {
                    response(false, 2)
                } else {
                    rejection()
                },
            ])),
        )
        .await;
        let fixture = fixture(&server).await;
        let turn = fixture.submit("Read native.txt").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
        assert_eq!(
            run.status,
            if succeeds {
                Status::Completed
            } else {
                Status::Failed
            },
            "{:?}",
            page.runs
        );
        if succeeds {
            assert!(text(&page).contains(server::ANSWER));
            assert!(run.error.is_none());
        } else {
            assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::Unavailable);
            assert!(
                run.error
                    .as_ref()
                    .unwrap()
                    .message
                    .contains("could not be verified")
            );
        }
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests.len(),
                3,
                "one recovery attempt after the rejected tool-loop request"
            );
            assert_eq!(signed(&requests[1].body), expected());
            assert!(signed(&requests[2].body).is_empty());
            assert!(!requests[2].body.to_string().contains(SIGNATURE));
            let without_reasoning = requests[1].body["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] != "reasoning")
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(requests[2].body["input"], json!(without_reasoning));
        }
        assert!(!serde_json::to_string(&page).unwrap().contains(SIGNATURE));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let errors = errors.0.lock().unwrap();
        for area in ["model", "run"] {
            assert_eq!(
                errors.iter().any(|value| value == area),
                !succeeds,
                "{errors:?}"
            );
        }
    }
}

#[derive(Clone, Default)]
struct Errors(Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Errors {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "sailry"
            || *event.metadata().level() != tracing::Level::ERROR
        {
            return;
        }
        #[derive(Default)]
        struct Fields {
            area: String,
            phase: String,
        }
        impl tracing::field::Visit for Fields {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                match field.name() {
                    "area" => self.area = value.into(),
                    "phase" => self.phase = value.into(),
                    _ => {}
                }
            }
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.phase == "failed" {
            self.0.lock().unwrap().push(fields.area);
        }
    }
}
