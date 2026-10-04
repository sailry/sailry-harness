//! Explicit, billable acceptance using an isolated profile and project.
use super::*;

#[tokio::test]
#[ignore = "requires explicit permission and incurs one real model request"]
async fn probes_one_request_without_tools() {
    use adk_core::{Content, GenerateContentConfig, Llm, LlmRequest};
    use adk_model::{
        openai::OpenAIReasoningEffort,
        opencode::{OpenCodeClient, OpenCodeConfig, OpenCodeService},
        retry::RetryConfig,
    };
    use futures::StreamExt;

    const MODEL: &str = "deepseek-v4.1-flash";
    let (provider, secret) = provider_fixture::load(MODEL);
    assert_eq!(provider.api, ModelApi::OpenCodeGo);
    assert_eq!(
        provider.endpoint.trim_end_matches('/'),
        "https://opencode.ai/zen/go/v1"
    );
    let config = OpenCodeConfig::new(OpenCodeService::Go, secret.expose(), MODEL)
        .with_base_url(&provider.endpoint)
        .with_session_id(uuid::Uuid::new_v4().to_string())
        .with_user_agent(concat!("Sailry/", env!("CARGO_PKG_VERSION")))
        .with_reasoning_effort(OpenAIReasoningEffort::Max)
        .with_retry_config(RetryConfig::disabled());
    let model = OpenCodeClient::new(config).expect("valid isolated configuration required");
    let mut request = LlmRequest::new(MODEL, vec![Content::new("user").with_text("Reply with OK")]);
    request.config = Some(GenerateContentConfig {
        max_output_tokens: Some(128),
        ..Default::default()
    });
    assert!(request.tools.is_empty());
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let mut stream = model.generate_content(request, true).await?;
        let mut responses = 0;
        while let Some(response) = stream.next().await {
            response?;
            responses += 1;
        }
        Ok::<_, adk_core::AdkError>(responses)
    })
    .await;
    match result {
        Ok(Ok(responses)) => println!("Single request completed: {responses} stream responses"),
        Ok(Err(error)) => panic!(
            "Single request failed: code={}, HTTP={:?}, category={}",
            error.code, error.details.upstream_status_code, error.category
        ),
        Err(_) => panic!("Single request timed out without retry"),
    }
}

#[tokio::test]
#[ignore = "requires SAILRY_ACCEPTANCE_PROFILE and incurs real model usage"]
async fn reads_file_with_deepseek_on_local_and_remote_nodes() {
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
