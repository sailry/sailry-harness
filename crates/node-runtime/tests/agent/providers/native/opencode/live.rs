//! Explicit, billable acceptance using an isolated profile and project.
use super::*;

#[tokio::test]
#[ignore = "requires explicit permission and incurs one real model request"]
async fn probes_one_request_without_tools() {
    use goose_provider_types::{
        base::{Provider, collect_stream},
        conversation::message::Message,
        model::ModelConfig,
    };
    struct Key(String);
    impl goose_providers::declarative::KeyResolver for Key {
        type Error = std::convert::Infallible;
        fn resolve_key(&self, _: &str) -> Result<String, Self::Error> {
            Ok(self.0.clone())
        }
    }
    const MODEL: &str = "deepseek-v4.1-flash";
    let (provider, secret) = provider_fixture::load(MODEL);
    assert_eq!(provider.api, ModelApi::OpenCodeGo);
    assert_eq!(
        provider.endpoint.trim_end_matches('/'),
        "https://opencode.ai/zen/go/v1"
    );
    let mut definition = goose_providers::declarative::fixed_provider_configs()
        .unwrap()
        .into_iter()
        .find(|config| config.name == "opencode_go")
        .unwrap();
    definition.base_url = provider.endpoint;
    let model = goose_providers::openai::from_declarative_config(
        definition,
        None,
        Key(secret.expose().to_owned()),
    )
    .unwrap()
    .build();
    let config = ModelConfig::new(MODEL).with_max_tokens(Some(128));
    let contents = [Message::user().with_text("Reply with OK")];
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let stream = model.stream(&config, "", &contents, &[]).await?;
        collect_stream(stream).await
    })
    .await;
    match result {
        Ok(Ok((message, _))) => assert!(!message.as_concat_text().is_empty()),
        Ok(Err(_)) => panic!("GDK model request failed"),
        Err(_) => panic!("GDK model request timed out"),
    }
}

#[tokio::test]
#[ignore = "requires SAILRY_ACCEPTANCE_PROFILE and incurs real model usage"]
async fn deepseek_reads_local_and_remote_files() {
    const MODEL: &str = "deepseek-v4.1-flash";
    let (source, secret) = provider_fixture::load(MODEL);
    assert_eq!(
        source.endpoint.trim_end_matches('/'),
        "https://opencode.ai/zen/go/v1"
    );
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote, ModelApi::OpenCodeGo, &source.endpoint).await;
        fixture.provider.models = source
            .models
            .iter()
            .filter(|model| model.id == MODEL)
            .cloned()
            .collect();
        fixture.provider.default_model = MODEL.into();
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::SaveProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
                secret: Some(secret.clone()),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = fixture.session.config.clone();
        config.model = MODEL.into();
        config.credential = provider.credential;
        config.effort = Effort::High;
        config.permission = Permission::Full;
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
        let expected = uuid::Uuid::new_v4().to_string();
        std::fs::write(fixture.root.join("acceptance.txt"), &expected).unwrap();
        let turn = fixture.submit("Use read_file to read acceptance.txt in the current project. Reply with only its exact contents. Do not change any files or call other tools.").await;
        let started = std::time::Instant::now();
        let page = loop {
            let page = history(&fixture.client, fixture.session.id).await;
            if page.runs.iter().any(|run| {
                run.turn == turn
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                break page;
            }
            if started.elapsed() > Duration::from_secs(180) {
                execute(&fixture.client, Command::StopTurn { turn }).await;
                break finished(&fixture.client, fixture.session.id, turn).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(
            text(&page).contains(&expected),
            "model must return the actual file contents"
        );
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::ToolCall { name, .. } if name == &plugin_tool("files", "read_file")))
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("acceptance.txt")).unwrap(),
            expected
        );
        println!(
            "{MODEL} remote={remote}: real streaming, file tool and continuation passed in {} ms",
            started.elapsed().as_millis()
        );
    }
}
