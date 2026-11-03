use super::*;
use sailry_protocol::media::{Action, Binding, Kind, Settings};
use std::sync::Mutex;

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";

#[derive(Clone, Copy, Debug)]
enum Outcome {
    Json,
    Stream,
    Rejected,
    Invalid,
    Interrupted,
    Cancelled,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generates_through_captured_authorization() {
    for remote in [false, true] {
        for outcome in [
            Outcome::Json,
            Outcome::Stream,
            Outcome::Rejected,
            Outcome::Invalid,
            Outcome::Interrupted,
            Outcome::Cancelled,
        ] {
            let companion = !matches!(outcome, Outcome::Json);
            let image_ready = Arc::new(tokio::sync::Notify::new());
            let image_seen = image_ready.clone();
            let held = Arc::new(tokio::sync::Notify::new());
            let ready = held.clone();
            let session = Arc::new(Mutex::new(None::<SessionId>));
            let captured = session.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/images/generations" {
                    assert!(matches!(outcome, Outcome::Rejected));
                    assert_eq!(
                        request.headers["authorization"],
                        "Bearer isolated-codex-image-companion"
                    );
                    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                    assert_eq!(body["model"], "companion-image");
                    return Reply::Json(json!({"data":[{"b64_json":PNG}]}));
                }
                if request.path != "/responses" {
                    return server::standard(request, 3600);
                }
                let options = Options::defaults(Authentication::ChatGptOAuth).unwrap();
                server::headers(request, &options, ACCESS);
                let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                let id = captured.lock().unwrap().unwrap().to_string();
                assert_eq!(body["model"], "fixture");
                assert_eq!(body["stream"], true);
                assert_eq!(body["store"], false);
                assert_eq!(body["prompt_cache_key"], id);
                assert_eq!(request.headers["session-id"], id);
                assert_eq!(request.headers["x-codex-routing-hint"], "model=fixture");
                if body["tools"].as_array().unwrap().iter()
                    .any(|tool| tool["type"] == "image_generation") {
                    assert_eq!(body["instructions"], "");
                    assert_eq!(body["input"], json!([{"type":"message","role":"user",
                        "content":[{"type":"input_text","text":"A pixel"}]}]));
                    assert_eq!(body["tools"], json!([{"type":"image_generation","output_format":"png"}]));
                    assert_eq!(body["tool_choice"], json!({"type":"image_generation"}));
                    assert!(body.get("max_tool_calls").is_none());
                    image_seen.notify_one();
                    let image = json!({"type":"image_generation_call","status":"completed","result":PNG});
                    return match outcome {
                        Outcome::Json => Reply::Json(json!({"status":"completed","output":[image]})),
                        Outcome::Stream => server::stream(vec![server::completed(json!([image]))], false),
                        Outcome::Rejected | Outcome::Invalid => {
                            let code = if matches!(outcome, Outcome::Rejected) { "unsupported_capability" } else { "invalid_request" };
                            let error = json!({"error":{"code":code,"type":"invalid_request_error",
                                "param":"tools", "message":format!("Fixture rejected {ACCESS} {REFRESH} {ACCOUNT}")}}).to_string();
                            Reply::Raw(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nX-Request-ID: req_image\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{error}",error.len()))
                        }
                        Outcome::Interrupted => server::stream(vec![json!({"type":"response.output_item.done","output_index":0,"item":image})], true),
                        Outcome::Cancelled => Reply::Hold,
                    };
                }
                server::input(request, &options, ACCESS);
                ready.notify_one();
                Reply::Hold
            })
            .await;
            let fixture = Fixture::packages(remote, &server, &["media"]).await;
            let provider = configured(&fixture, None).await;
            assert!(provider.models[0].generates.is_empty());
            if companion {
                let mut companion = provider.clone();
                companion.id = ProviderId::new();
                companion.revision = 0;
                companion.api = ModelApi::ChatCompletions;
                companion.authentication = Authentication::ApiKey;
                companion.endpoint = server.endpoint.clone();
                companion.oauth = None;
                companion.credential = None;
                companion.models[0].id = "companion-image".into();
                companion.default_model = "companion-image".into();
                let Output::Provider(companion) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::SaveProvider {
                        expected_revision: 0,
                        provider: companion,
                        secret: Some(Secret::new("isolated-codex-image-companion".into())),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("companion provider expected")
                };
                fixture
                    .client
                    .execute(
                        fixture.client.prepare(Command::SaveMediaSettings(Settings {
                            revision: 0,
                            bindings: [(
                                Kind::Image,
                                Some(Binding {
                                    provider: companion.id,
                                    model: "companion-image".into(),
                                }),
                            )]
                            .into(),
                        })),
                    )
                    .await
                    .unwrap();
            }
            let base = super::super::execution::session(&fixture, &provider).await;
            let Output::Session(active) = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: base.id,
                    expected_revision: base.revision,
                    config: SessionConfig {
                        permission: Permission::Full,
                        ..base.config
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            *session.lock().unwrap() = Some(active.id);
            let Output::Snapshot(snapshot) = fixture
                .client
                .execute(fixture.client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let root = std::path::PathBuf::from(
                &snapshot
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.id == active.worktree)
                    .unwrap()
                    .path,
            );
            let Output::Plugin(package) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPlugin {
                    name: "media".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("media package expected")
            };
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            let (_, turn) = super::super::execution::submit(&fixture.client, &active).await;
            assert!(turn.plugins.contains(&package.summary.reference()));
            tokio::time::timeout(Duration::from_secs(10), held.notified())
                .await
                .expect("main model request did not reach the held response");
            let path = "assets/generated/codex.png";
            let request = fixture
                .client
                .prepare(Command::UseMedia {
                    turn: turn.id,
                    session: active.id,
                    worktree: active.worktree,
                    action: Action::Image {
                        prompt: "A pixel".into(),
                        path: path.into(),
                    },
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: Some(turn.id),
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(active.worktree),
                    session: Some(active.id),
                });
            let generated = if matches!(outcome, Outcome::Cancelled) {
                let generate = fixture.client.execute(request);
                let cancel = async {
                    tokio::time::timeout(Duration::from_secs(10), image_ready.notified())
                        .await
                        .expect("image request was not submitted");
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
                        .await
                        .unwrap();
                };
                let (result, ()) = tokio::join!(generate, cancel);
                result
            } else {
                fixture.client.execute(request).await
            };

            if matches!(outcome, Outcome::Json | Outcome::Stream | Outcome::Rejected) {
                assert!(
                    matches!(generated.unwrap(), Output::Media(value) if value["path"] == path)
                );
                assert_eq!(
                    std::fs::read(root.join(path)).unwrap(),
                    base64::engine::general_purpose::STANDARD
                        .decode(PNG)
                        .unwrap()
                );
            } else {
                let error = generated.unwrap_err();
                no_secrets(&error);
                assert_eq!(
                    error.code,
                    match outcome {
                        Outcome::Invalid => ErrorCode::Unavailable,
                        Outcome::Interrupted => ErrorCode::OutcomeUnknown,
                        Outcome::Cancelled => ErrorCode::Cancelled,
                        _ => unreachable!(),
                    }
                );
                if matches!(outcome, Outcome::Invalid) {
                    assert!(error.message.contains("param tools"));
                    assert!(error.message.contains("req_image"));
                }
                assert!(!root.join(path).exists());
            }
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(
                    requests
                        .iter()
                        .filter(|request| request.path == "/responses")
                        .count(),
                    2
                );
                assert_eq!(
                    requests
                        .iter()
                        .filter(|request| request.path == "/images/generations")
                        .count(),
                    usize::from(matches!(outcome, Outcome::Rejected))
                );
            }
            if !matches!(outcome, Outcome::Cancelled) {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
                    .await
                    .unwrap();
            }
            super::super::execution::finished(&fixture.client, &turn, Status::Cancelled).await;
            server
                .wait_closed(if matches!(outcome, Outcome::Cancelled) {
                    2
                } else {
                    1
                })
                .await;
            fixture.close().await;
        }
    }
}
