use super::*;
use sailry_protocol::conversation::{Page, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
#[path = "execution/oauth.rs"]
mod oauth;
#[path = "execution/renewals.rs"]
mod renewals;
#[path = "execution/transfers.rs"]
mod transfers;

const ROTATED: &str = "isolated-rotated-refresh";

fn token(expires: u64) -> String {
    format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(json!({"exp":expires,"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}}).to_string()))
}

pub(super) fn exchanged(authentication: Authentication, fresh: bool) -> serde_json::Value {
    let expires = if fresh {
        future_expiry()
    } else {
        future_expiry() - 3570
    };
    match authentication {
        Authentication::ChatGpt => {
            json!({"access_token":token(expires),"refresh_token": if fresh {ROTATED} else {REFRESH}})
        }
        Authentication::Copilot => {
            json!({"token": if fresh {"renewed-access"} else {ACCESS},"expires_at":expires,"endpoints":{"api":"https://api.individual.githubcopilot.com"}})
        }
        _ => unreachable!(),
    }
}

fn exchange(request: &discovery_support::Request) -> bool {
    request.path == "/oauth/token" || request.path == "/copilot_internal/v2/token"
}

fn generation(
    request: &discovery_support::Request,
    authentication: Authentication,
    tools: bool,
) -> Reply {
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "fixture");
    assert_eq!(body["stream"], true);
    assert!(!request.body.contains(REFRESH));
    assert!(!request.body.contains(GITHUB));
    let api = if request.path == "/responses" {
        ModelApi::Responses
    } else {
        ModelApi::ChatCompletions
    };
    let continuation = if api == ModelApi::Responses {
        body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["type"] == "function_call_output")
    } else {
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["role"] == "tool")
    };
    if authentication == Authentication::ChatGpt {
        assert_eq!(api, ModelApi::Responses);
        assert_eq!(request.headers["chatgpt-account-id"], "fixture-account");
        assert_eq!(request.headers["originator"], "sailry");
        assert!(request.headers["session_id"].parse::<SessionId>().is_ok());
        assert!(request.headers["authorization"].starts_with("Bearer e30."));
        assert_eq!(body["store"], false);
        assert!(body["instructions"].is_string());
        assert!(
            body["include"]
                .as_array()
                .unwrap()
                .contains(&json!("reasoning.encrypted_content"))
        );
        assert!(body.get("max_output_tokens").is_none());
    } else {
        assert_eq!(request.headers["authorization"], "Bearer renewed-access");
        assert_eq!(
            request.headers["x-initiator"],
            if continuation { "agent" } else { "user" }
        );
        assert_eq!(request.headers["copilot-integration-id"], "vscode-chat");
        assert!(request.headers["x-request-id"].parse::<RequestId>().is_ok());
    }
    let call = tools && !continuation;
    let digest = blake3::hash(b"files").to_hex();
    let tool = format!("plugin_{}_read_file", &digest[..16]);
    let event = if api == ModelApi::Responses {
        let output = if call {
            json!([{"type":"function_call","id":"item-fixture","call_id":"call-fixture","name":tool,"arguments":"{\"path\":\"fixture.txt\"}","status":"completed"}])
        } else {
            json!([{"type":"message","id":"message-fixture","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Authorized response","annotations":[]}]}])
        };
        let completed = json!({"type":"response.completed","sequence_number":1,"response":{"id":"response-fixture","object":"response","created_at":1,"status":"completed","model":"fixture","output":output}});
        if call {
            format!("data: {completed}\n\n")
        } else {
            let delta = json!({"type":"response.output_text.delta","sequence_number":0,"item_id":"message-fixture","output_index":0,"content_index":0,"delta":"Authorized response","logprobs":[]});
            format!("data: {delta}\n\ndata: {completed}\n\n")
        }
    } else {
        let delta = if call {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"call-fixture","type":"function","function":{"name":tool,"arguments":"{\"path\":\"fixture.txt\"}"}}]})
        } else {
            json!({"role":"assistant","content":"Authorized response"})
        };
        let first = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        let last = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":if call {"tool_calls"} else {"stop"}}]});
        format!("data: {first}\n\ndata: {last}\n\n")
    };
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{event}data: [DONE]\n\n"
    ))
}

pub(super) async fn signed_in(
    fixture: &Fixture,
    authentication: Authentication,
    api: ModelApi,
) -> Provider {
    configured(fixture, authentication, api, None).await
}

async fn configured(
    fixture: &Fixture,
    authentication: Authentication,
    api: ModelApi,
    options: Option<sailry_protocol::conversation::oauth::Options>,
) -> Provider {
    let mut provider = provider(&fixture.client, authentication).await;
    provider.oauth = options;
    provider.models = vec![sailry_protocol::conversation::Model {
        id: "fixture".into(),
        context: 8192,
        output: 512,
        vision: true,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![],
        efforts: vec![],
        custom_efforts: false,
        default_effort: Effort::Default,
    }];
    {
        provider.api = api;
        let Output::Provider(saved) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        provider = saved;
    }
    let (_, attempt) = begin(&fixture.client, &provider).await;
    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
    assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
    let Output::Providers(providers) = fixture
        .client
        .execute(fixture.client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    providers
        .into_iter()
        .find(|value| value.id == provider.id)
        .unwrap()
}

async fn session(fixture: &Fixture, provider: &Provider) -> Session {
    let root = fixture.directory.path().join(ProjectId::new().to_string());
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("fixture.txt"), "Isolated tool content").unwrap();
    let Output::Project(project) = fixture
        .client
        .execute(fixture.client.prepare(Command::RegisterProject {
            name: "Authorization fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture".into(),
                effort: Effort::Default,
                mode: sailry_protocol::WorkMode::Code,
                permission: Permission::Ask,
                credential: provider.credential.clone(),
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

async fn submit(client: &Client, session: &Session) -> (Request, QueuedTurn) {
    let request = client.prepare(Command::SubmitTurn {
        session: session.id,
        expected_revision: session.revision,
        message: "Authorization fixture".into(),
    });
    let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    (request, turn)
}

async fn finished(client: &Client, turn: &QueuedTurn, status: Status) -> Page {
    finished_within(client, turn, status, Duration::from_secs(10)).await
}

async fn finished_within(
    client: &Client,
    turn: &QueuedTurn,
    status: Status,
    budget: Duration,
) -> Page {
    tokio::time::timeout(budget, async {
        loop {
            let Output::Conversation(history) = client
                .execute(client.prepare(Command::ReadConversation {
                    session: turn.session,
                    before: None,
                    limit: 10,
                }))
                .await
                .unwrap()
            else {
                panic!("history expected")
            };
            if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn.id)
                && matches!(
                    run.status,
                    Status::Completed | Status::Failed | Status::Cancelled | Status::Interrupted
                )
            {
                assert_eq!(run.status, status, "{run:?}");
                no_secrets(&history);
                assert!(!serde_json::to_string(&history).unwrap().contains(ROTATED));
                return history.page;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("authorization execution deadline")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runs_tools_after_refresh() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if exchange(request) {
                    let fresh = count.fetch_add(1, Ordering::SeqCst) > 0;
                    if fresh && authentication == Authentication::ChatGpt {
                        assert!(request.body.contains("grant_type=refresh_token"));
                        assert!(request.body.contains(REFRESH));
                    }
                    Reply::Json(exchanged(authentication, fresh))
                } else if matches!(request.path.as_str(), "/responses" | "/chat/completions") {
                    generation(request, authentication, true)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = signed_in(&fixture, authentication, api).await;
            let session = session(&fixture, &provider).await;
            let (request, turn) = submit(&fixture.client, &session).await;
            let page = finished(&fixture.client, &turn, Status::Completed).await;
            let history = serde_json::to_string(&page).unwrap();
            assert!(history.contains("Authorized response"));
            assert!(history.contains("Isolated tool content"), "{history}");
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            assert_eq!(server.requests.lock().unwrap().len(), 7);
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn)
            );
            fixture.close().await;
        }
    }
}
