use super::*;
use sailry_protocol::media::{Action, Generation};
use std::sync::Mutex;

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishes_only_completed_image() {
    for remote in [false, true] {
        for outcome in ["completed", "eof", "incomplete"] {
            let session = Arc::new(Mutex::new(None::<SessionId>));
            let captured = session.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
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
                    return Reply::Hold;
                }
                assert_eq!(body["tools"], json!([{"type":"image_generation","output_format":"png"}]));
                assert!(body["instructions"].is_string());
                let item = json!({"type":"image_generation_call","id":"codex-image","status":"completed","result":PNG});
                if outcome == "eof" {
                    return server::stream(vec![json!({"type":"response.output_item.done","sequence_number":1,"output_index":0,"item":item})], false);
                }
                let mut completed = server::completed(json!([item]));
                if outcome == "incomplete" {
                    completed["type"] = json!("response.incomplete");
                    completed["response"]["status"] = json!("incomplete");
                }
                server::stream(vec![completed], true)
            }).await;
            let fixture = Fixture::new(remote, &server).await;
            let mut provider = configured(&fixture, None).await;
            provider.models[0].generates = vec![Generation::ImageTool];
            let Output::Provider(provider) = fixture
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
            let before = server.requests.lock().unwrap().len();
            let (_, turn) = super::super::execution::submit(&fixture.client, &active).await;
            assert!(turn.plugins.contains(&package.summary.reference()));
            server.wait_requests(before + 1).await;
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
            if outcome == "completed" {
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
                assert_eq!(error.code, ErrorCode::Unavailable);
                no_secrets(&error);
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
