use super::*;
use sailry_protocol::media::{Action as MediaAction, Binding, Generation, Kind, Settings, Source};
use std::sync::Arc;
use tokio::sync::Notify;

#[path = "../../agent/media/server.rs"]
mod server;

async fn running(
    client: &Client,
    context: &mut Context,
    endpoint: &str,
    primary: &model_fixture::Server,
) -> TurnId {
    let selected = completion::configure(client, primary.endpoint.clone()).await;
    let primary_id = selected.split_once('/').unwrap().0.parse().unwrap();
    let Output::Providers(providers) = execute(client, Command::ListProviders).await else {
        panic!("providers expected")
    };
    let mut provider = providers
        .into_iter()
        .find(|provider| provider.id == primary_id)
        .unwrap();
    provider.id = ProviderId::new();
    provider.revision = 0;
    provider.endpoint = endpoint.into();
    provider.name = "Media fixture".into();
    provider.models[0].vision = true;
    provider.models[0].generates = vec![Generation::Image, Generation::Video];
    execute(
        client,
        Command::PutProvider {
            provider: provider.clone(),
            expected_revision: 0,
        },
    )
    .await;
    execute(
        client,
        Command::SaveMediaSettings(Settings {
            revision: 0,
            bindings: [Kind::Vision, Kind::Image, Kind::Video]
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        Binding {
                            provider: provider.id,
                            model: "fixture".into(),
                        },
                    )
                })
                .collect(),
        }),
    )
    .await;
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|worktree| Some(worktree.id) == context.worktree)
        .unwrap()
        .project;
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project,
            worktree: context.worktree,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: primary_id,
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Full,
                credential: None,
            }),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    context.session = Some(session.id);
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Hold this turn while its scoped media operation runs".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    context.turn = Some(turn.id);
    execute(client, Command::StartQueuedTurn { turn: turn.id }).await;
    primary.wait_count(1).await;
    turn.id
}

fn request(client: &Client, context: &Context, action: MediaAction) -> Request {
    client
        .prepare(Command::UseMedia {
            turn: context.turn.unwrap(),
            session: context.session.unwrap(),
            worktree: context.worktree.unwrap(),
            action,
        })
        .with_plugin(context.clone())
}

fn image(path: &str) -> MediaAction {
    MediaAction::Image {
        prompt: "A pixel".into(),
        path: path.into(),
    }
}

async fn outcome(client: &Client, request: &Request) -> Result<Output, Fault> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let RequestOutcome::Completed(result) = client.outcome(request).await.unwrap() {
                break *result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn recovers_without_replaying_jobs() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        fs::write(root.join("input.png"), server::image()).unwrap();
        let mut context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[
                Action::InspectMedia,
                Action::GenerateImage,
                Action::GenerateVideo,
            ],
        )
        .await;
        let primary =
            model_fixture::Server::held(Arc::new(Notify::new()), Arc::new(Notify::new())).await;
        let media = server::Server::start().await;
        let turn = running(&client, &mut context, &media.endpoint, &primary).await;
        let inspected = client
            .execute(request(
                &client,
                &context,
                MediaAction::Inspect {
                    prompt: "Describe".into(),
                    source: Source::Path("input.png".into()),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(inspected,Output::Media(value) if value["text"]=="One bright pixel"));
        let original = request(&client, &context, image("generated/pixel.png"));
        let pending = client.dispatch(original.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let result = outcome(&client, &original).await.unwrap();
        assert!(matches!(&result,Output::Media(value) if value["path"]=="generated/pixel.png"));
        assert_eq!(
            fs::read(root.join("generated/pixel.png")).unwrap(),
            server::image()
        );
        let mut changed = original.clone();
        let Command::UseMedia { action, .. } = &mut changed.command else {
            unreachable!()
        };
        *action = image("generated/different.png");
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(request(&client, &context, image("generated/pixel.png")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            media
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path == "/v1/images/generations")
                .count(),
            1
        );
        let video = request(
            &client,
            &context,
            MediaAction::Video {
                prompt: "pending".into(),
                path: "generated/pending.mp4".into(),
            },
        );
        let admitted = client.dispatch(video.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !media
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/v1/videos")
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(client.execute(original.clone()).await.unwrap(), result);
        assert_eq!(
            client
                .execute(request(&client, &context, image("generated/disabled.png")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        assert_eq!(client.execute(original.clone()).await.unwrap(), result);
        execute(&client, Command::StopTurn { turn }).await;
        let stopped = outcome(&client, &video).await.unwrap_err();
        assert_eq!(stopped.code, ErrorCode::Cancelled);
        assert!(!root.join("generated/pending.mp4").exists());
        let count = media.requests.lock().unwrap().len();
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(original).await.unwrap(), result);
        assert_eq!(client.execute(video).await.unwrap_err(), stopped);
        assert_eq!(media.requests.lock().unwrap().len(), count);
        assert!(!root.join("generated/different.png").exists());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn validates_before_provider_calls() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::InspectMedia],
        )
        .await;
        let primary =
            model_fixture::Server::held(Arc::new(Notify::new()), Arc::new(Notify::new())).await;
        let media = server::Server::start().await;
        let turn = running(&client, &mut context, &media.endpoint, &primary).await;
        assert_eq!(
            client
                .execute(request(&client, &context, image("denied.png")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for field in ["turn", "session", "worktree"] {
            let mut denied = request(
                &client,
                &context,
                MediaAction::Inspect {
                    prompt: "Inspect".into(),
                    source: Source::Path("input.png".into()),
                },
            );
            let Command::UseMedia {
                turn,
                session,
                worktree,
                ..
            } = &mut denied.command
            else {
                unreachable!()
            };
            match field {
                "turn" => *turn = TurnId::new(),
                "session" => *session = SessionId::new(),
                _ => *worktree = WorktreeId::new(),
            }
            assert_eq!(
                client.execute(denied).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let escaped = request(
            &client,
            &context,
            MediaAction::Inspect {
                prompt: "Inspect".into(),
                source: Source::Path("../outside.png".into()),
            },
        );
        let fault = client.execute(escaped.clone()).await.unwrap_err();
        assert_eq!(fault.code, ErrorCode::InvalidRequest);
        assert_eq!(outcome(&client, &escaped).await, Err(fault));
        assert!(media.requests.lock().unwrap().is_empty());
        execute(&client, Command::StopTurn { turn }).await;
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn disabled_catalog_preserves_scope() {
    for remote in [false, true] {
        let (_directory, node, controller, client, worktree) = fixture(remote).await;
        let package = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "media".into(),
                },
            )
            .await,
        );
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let package = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "media".into(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let mut context = Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Settings,
            package: package.summary.reference(),
            session: None,
            worktree: None,
        };
        let session = scopes::session(&client, worktree).await;
        let Output::Providers(mut providers) = execute(&client, Command::ListProviders).await
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.models[0].vision = true;
        provider.models[0].generates = vec![Generation::Image];
        execute(
            &client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider: provider.clone(),
            },
        )
        .await;
        let catalog = client
            .execute(
                client
                    .prepare(Command::ListMediaModels)
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        let Output::MediaModels(candidates) = &catalog else {
            panic!("catalog expected")
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider, session.config.provider);
        assert_eq!(candidates[0].kinds, vec![Kind::Vision, Kind::Image]);
        let encoded = serde_json::to_string(&catalog).unwrap();
        assert!(!encoded.contains("endpoint"));
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("127.0.0.1"));
        let saved = client
            .prepare(Command::SaveMediaSettings(Settings {
                revision: 0,
                bindings: [(
                    Kind::Image,
                    Binding {
                        provider: provider.id,
                        model: provider.models[0].id.clone(),
                    },
                )]
                .into(),
            }))
            .with_plugin(context.clone());
        let pending = client.dispatch(saved.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        let result = pending.completion.await.unwrap().unwrap();
        assert_eq!(client.execute(saved).await.unwrap(), result);
        let Output::MediaSettings(settings) = client
            .execute(
                client
                    .prepare(Command::ReadMediaSettings)
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert_eq!(settings.revision, 1);
        let invalid = client
            .prepare(Command::SaveMediaSettings(Settings {
                revision: 1,
                bindings: [(
                    Kind::Video,
                    Binding {
                        provider: provider.id,
                        model: provider.models[0].id.clone(),
                    },
                )]
                .into(),
            }))
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(invalid).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        context.surface = Default::default();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ListMediaModels)
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
