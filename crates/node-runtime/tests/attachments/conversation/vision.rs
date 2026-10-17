use super::*;
use base64::Engine as _;
use sailry_protocol::media::{Binding, Kind, Settings};
use serde_json::json;

#[path = "../../agent/media/server.rs"]
mod companion;

async fn select(client: &Client, session: &Session, model: &str) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: SessionConfig {
                model: model.into(),
                permission: Permission::Full,
                ..session.config.clone()
            },
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

fn last_status(history: &Page, turn: TurnId) -> Status {
    history
        .runs
        .iter()
        .find(|run| run.turn == turn)
        .unwrap()
        .status
}

#[tokio::test]
async fn preserves_images_across_model_changes() {
    let fixture = Fixture::packages(&["media"]).await;
    let server = server::Server::scripted_tools(vec![]).await;
    let companion = companion::Server::start().await;
    let first_bytes = png();
    let second_bytes = companion::image();
    let first_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&first_bytes)
    );
    let second_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&second_bytes)
    );
    for client in fixture.clients() {
        let (base, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        for (name, tools) in [("text-tools", true), ("text-only", false)] {
            let mut metadata = provider.models[0].clone();
            metadata.id = name.into();
            metadata.vision = false;
            metadata.tools = tools;
            metadata.capabilities.as_mut().unwrap().vision = Some(false);
            metadata.capabilities.as_mut().unwrap().tools = Some(tools);
            provider.models.push(metadata);
        }
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut vision_provider = provider.clone();
        vision_provider.id = ProviderId::new();
        vision_provider.revision = 0;
        vision_provider.endpoint = companion.endpoint.clone();
        vision_provider.models.truncate(1);
        let Output::Provider(vision_provider) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: 0,
                provider: vision_provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let Output::MediaSettings(settings) = client
            .execute(client.prepare(Command::ReadMediaSettings))
            .await
            .unwrap()
        else {
            panic!("media settings expected")
        };
        client
            .execute(
                client.prepare(Command::SaveMediaSettings(Settings {
                    revision: settings.revision,
                    bindings: [(
                        Kind::Vision,
                        Some(Binding {
                            provider: vision_provider.id,
                            model: "fixture".into(),
                        }),
                    )]
                    .into(),
                })),
            )
            .await
            .unwrap();
        let Output::Session(current) = client
            .execute(client.prepare(Command::CreateSession {
                project: base.project,
                worktree: Some(base.worktree),
                config: Some(SessionConfig {
                    permission: Permission::Full,
                    ..base.config.clone()
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let first = upload(
            &client,
            fixture.worktree,
            "first.png",
            "image/png",
            &first_bytes,
        )
        .await;
        let second = upload(
            &client,
            fixture.worktree,
            "second.png",
            "image/png",
            &second_bytes,
        )
        .await;
        let before = server.requests.lock().unwrap().len();
        let sent = queue(
            &client,
            &current,
            input("Inspect the first image", &[&first]),
        )
        .await;
        assert_eq!(
            last_status(&run(&client, &sent).await, sent.id),
            Status::Completed
        );
        let request = server.requests.lock().unwrap().last().unwrap().to_string();
        assert!(request.contains(&first_url));

        let current = select(&client, &current, "text-tools").await;
        server.replace_tools(vec![(
            server::plugin_tool("media", "inspect_image"),
            json!({"prompt":"Describe the earlier image", "attachment":first.id}),
        )]);
        let companion_before = companion.requests.lock().unwrap().len();
        let inspect = queue(
            &client,
            &current,
            "Inspect the earlier image with the configured tool".into(),
        )
        .await;
        let history = run(&client, &inspect).await;
        assert_eq!(last_status(&history, inspect.id), Status::Completed);
        assert!(history.entries.iter().filter(|entry| entry.turn == inspect.id).flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolResult { result, .. } if result.to_string().contains("One bright pixel"))));
        {
            let requests = companion.requests.lock().unwrap();
            assert_eq!(requests.len(), companion_before + 1);
            assert!(requests.last().unwrap().1.to_string().contains(&first_url));
        }
        {
            let requests = server.requests.lock().unwrap();
            let request = &requests[before + 1];
            assert_eq!(request["model"], "text-tools");
            let serialized = request.to_string();
            assert!(serialized.contains(&first.id.to_string()));
            assert!(!serialized.contains(&first_url));
        }
        server.replace_tools(vec![]);
        let current = select(&client, &current, "text-only").await;
        let requests_before = server.requests.lock().unwrap().len();
        let failed = queue(&client, &current, input("Inspect a new image", &[&second])).await;
        let history = run(&client, &failed).await;
        let failed_run = history
            .runs
            .iter()
            .find(|run| run.turn == failed.id)
            .unwrap();
        assert_eq!(failed_run.status, Status::Failed);
        let error = failed_run.error.as_ref().unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "attachment requires tools or a supported native model input"
        );
        assert!(!error.message.contains("attachment is unavailable"));
        assert_eq!(server.requests.lock().unwrap().len(), requests_before);
        assert_eq!(
            companion.requests.lock().unwrap().len(),
            companion_before + 1
        );

        let plain = queue(
            &client,
            &current,
            "Continue using the earlier description".into(),
        )
        .await;
        let history = run(&client, &plain).await;
        assert_eq!(last_status(&history, plain.id), Status::Completed);
        let request = server.requests.lock().unwrap().last().unwrap().to_string();
        assert!(!request.contains(&first_url));
        assert!(!request.contains(&second_url));
        assert!(request.contains("One bright pixel"));
        assert!(
            request
                .contains("Contents not included; the current model cannot read this attachment")
        );

        let current = select(&client, &current, "fixture").await;
        let restored = queue(&client, &current, "Inspect both original images".into()).await;
        // A later controller choice cannot rewrite a queued model capture.
        let _changed = select(&client, &current, "text-only").await;
        assert_eq!(restored.config.model, "fixture");
        let history = run(&client, &restored).await;
        assert_eq!(last_status(&history, restored.id), Status::Completed);
        let request = server.requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(request["model"], "fixture");
        let serialized = request.to_string();
        assert!(serialized.contains(&first_url));
        assert!(serialized.contains(&second_url));
        assert!(!serialized.contains("sailry-attachment://"));
        assert_eq!(server.requests.lock().unwrap().len(), before + 5);
        assert_eq!(
            companion.requests.lock().unwrap().len(),
            companion_before + 1
        );
        for attachment in [&first, &second] {
            assert!(
                history
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| *part == Part::Attachment(attachment.clone()))
            );
        }
        assert_eq!(download(&client, &first).await, first_bytes);
        assert_eq!(download(&client, &second).await, second_bytes);
        assert_eq!(
            first.spec.revision,
            blake3::hash(&first_bytes).to_hex().to_string()
        );
        assert_eq!(
            second.spec.revision,
            blake3::hash(&second_bytes).to_hex().to_string()
        );
    }
    fixture.close().await;
}
