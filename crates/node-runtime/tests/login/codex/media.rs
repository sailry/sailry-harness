use super::*;
use sailry_protocol::media::{Action, Binding, Kind, Settings};
use std::sync::Mutex;

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishes_only_completed_image() {
    for remote in [false, true] {
        for outcome in [
            "completed",
            "empty_tail",
            "unsupported",
            "eof",
            "incomplete",
        ] {
            let held = Arc::new(tokio::sync::Notify::new());
            let ready = held.clone();
            let session = Arc::new(Mutex::new(None::<SessionId>));
            let captured = session.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/images/generations" {
                    assert_eq!(request.headers["authorization"], "Bearer isolated-codex-image-companion");
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
                if !body["tools"].as_array().unwrap().iter().any(|tool| tool["type"] == "image_generation") {
                    server::input(request, &options, ACCESS);
                    ready.notify_one();
                    return Reply::Hold;
                }
                assert_eq!(body["tools"], json!([{"type":"image_generation","output_format":"png"}]));
                assert!(body["instructions"].is_string());
                if outcome == "unsupported" {
                    let body = json!({"error":{"message":"Fixture capability unavailable","type":"invalid_request_error","code":"unsupported_capability","param":"tools"}}).to_string();
                    return Reply::Raw(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
                }
                let item = json!({"type":"image_generation_call","id":"codex-image","status":"completed","result":PNG,"output_format":"png"});
                let done = json!({"type":"response.output_item.done","sequence_number":1,"output_index":0,"item":item});
                if outcome == "eof" {
                    return server::stream(vec![done], false);
                }
                if outcome == "empty_tail" {
                    let message = json!({"type":"message","id":"codex-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Generated","annotations":[]}]});
                    let mut completed = server::completed(json!([]));
                    completed["sequence_number"] = json!(3);
                    return server::stream(vec![
                        done,
                        json!({"type":"response.output_item.done","sequence_number":2,"output_index":1,"item":message}),
                        completed,
                    ], true);
                }
                let mut completed = server::completed(json!([item]));
                if outcome == "incomplete" {
                    completed["type"] = json!("response.incomplete");
                    completed["response"]["status"] = json!("incomplete");
                }
                server::stream(vec![completed], true)
            }).await;
            let fixture = Fixture::packages(remote, &server, &["media"]).await;
            let provider = configured(&fixture, None).await;
            assert!(provider.models[0].generates.is_empty());
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
            let generated = fixture
                .client
                .execute(
                    fixture
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
                        }),
                )
                .await;
            if matches!(outcome, "completed" | "empty_tail" | "unsupported") {
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
                    if outcome == "eof" {
                        ErrorCode::OutcomeUnknown
                    } else {
                        ErrorCode::Unavailable
                    },
                    "remote={remote} outcome={outcome} message={}",
                    error.message
                );
                assert!(
                    !root.join(path).exists(),
                    "noncompleted image must not be published"
                );
            }
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/responses")
                    .count(),
                2,
                "image generation must not replay or change model routes"
            );
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/images/generations")
                    .count(),
                usize::from(outcome == "unsupported"),
                "only a definite capability rejection may use the companion"
            );
            fixture
                .client
                .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
                .await
                .unwrap();
            super::super::execution::finished(&fixture.client, &turn, Status::Cancelled).await;
            server.wait_closed(1).await;
            fixture.close().await;
        }
    }
}
