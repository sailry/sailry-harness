use super::*;
use sailry_protocol::conversation::Status;

const ANSWER: &str = "Authorized plugin response";

fn response(api: ModelApi) -> Reply {
    let mut body = String::new();
    let events = match api {
        ModelApi::ChatCompletions => vec![json!({
            "id":"fixture", "object":"chat.completion.chunk", "created":0,
            "model":"fixture", "choices":[{"index":0,"delta":{"content":ANSWER},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":8,"completion_tokens":4,"total_tokens":12}
        })],
        ModelApi::Responses => vec![json!({
            "type":"response.completed", "sequence_number":1,
            "response":{"id":"fixture", "object":"response", "created_at":0,
                "model":"fixture", "status":"completed", "output":[{
                    "type":"message","id":"answer","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":ANSWER,"annotations":[]}]
                }]}
        })],
        ModelApi::Anthropic => vec![
            json!({"type":"message_start","message":{"id":"fixture","type":"message","role":"assistant","model":"fixture","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":ANSWER}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ],
        _ => panic!("native fixture protocol expected"),
    };
    for event in events {
        if api == ModelApi::Anthropic {
            body.push_str(&format!("event: {}\n", event["type"].as_str().unwrap()));
        }
        body.push_str(&format!("data: {event}\n\n"));
    }
    if api == ModelApi::ChatCompletions {
        body.push_str("data: [DONE]\n\n");
    }
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn injects_only_declared_headers() {
    for remote in [false, true] {
        for api in [
            ModelApi::ChatCompletions,
            ModelApi::Responses,
            ModelApi::Anthropic,
        ] {
            for header in ["x-provider-token", "authorization", "x-api-key"] {
                let server = Server::start_with_request(api, move |request| {
                    let path = request.path.trim_start_matches("/v1");
                    if path == "/token" {
                        return tokens(true);
                    }
                    assert_eq!(request.headers[header], format!("Token {FRESH}"));
                    for other in ["authorization", "x-api-key"] {
                        if other != header {
                            assert!(
                                !request.headers.contains_key(other),
                                "undeclared authentication header: {other}"
                            );
                        }
                    }
                    assert!(
                        request
                            .headers
                            .values()
                            .all(|value| !value.contains("oauth-placeholder"))
                    );
                    if api == ModelApi::Anthropic {
                        assert_eq!(request.headers["anthropic-version"], "2023-06-01");
                    }
                    if path.starts_with("/models") {
                        if api == ModelApi::Anthropic {
                            return Reply::Json(json!({"data":[{
                                "id":"fixture","type":"model","display_name":"Fixture",
                                "max_input_tokens":8192,"max_tokens":512,
                                "tool_call":true,"reasoning":false
                            }],"has_more":false}));
                        }
                        return Reply::Json(catalog::models(Authentication::Plugin));
                    }
                    assert!(
                        matches!(path, "/chat/completions" | "/responses" | "/messages"),
                        "unexpected native request: {}",
                        request.path
                    );
                    response(api)
                })
                .await;
                let fixture = Fixture::empty(remote, &server).await;
                let provider = configured_channel(
                    &fixture,
                    true,
                    json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}),
                    None,
                    Some((api, json!([{"header":header,"template":"Token {access}"}]))),
                ).await;
                let (_, attempt) = begin(&fixture.client, &provider).await;
                let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
                let update = terminal(&mut updates).await;
                assert_eq!(
                    update.state,
                    login::State::Connected,
                    "remote={remote}, api={api:?}, header={header}"
                );
                assert_eq!(
                    update.model_error, None,
                    "remote={remote}, api={api:?}, header={header}"
                );
                let provider = saved(&fixture.client, provider.id).await;
                let session = super::super::execution::session(&fixture, &provider).await;
                let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
                let page =
                    super::super::execution::finished(&fixture.client, &turn, Status::Completed)
                        .await;
                private(&page);
                assert!(serde_json::to_string(&page).unwrap().contains(ANSWER));
                let generated = server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| {
                        request.path.ends_with("/chat/completions")
                            || request.path.ends_with("/responses")
                            || request.path.ends_with("/messages")
                    })
                    .count();
                assert_eq!(generated, 1);
                fixture.close().await;
            }
        }
    }
}
