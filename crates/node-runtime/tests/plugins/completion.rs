use super::*;
use plugin::Action;
#[path = "completion/provider.rs"]
mod provider;

pub(super) async fn configure(client: &Client, endpoint: String) -> String {
    configure_api(client, endpoint, conversation::ModelApi::ChatCompletions).await
}

async fn configure_api(client: &Client, endpoint: String, api: conversation::ModelApi) -> String {
    for name in ["context7", "github"] {
        let package = info(execute(client, Command::ReadPlugin { name: name.into() }).await);
        if package.summary.enabled {
            execute(
                client,
                Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                },
            )
            .await;
        }
    }
    let provider = conversation::Provider {
        oauth: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Plugin model fixture".into(),
        api,
        authentication: Authentication::ApiKey,
        endpoint,
        enabled: true,
        credential: None,
        default_model: "fixture".into(),
        models: vec![conversation::Model {
            id: "fixture".into(),
            context: 128_000,
            output: 4096,
            vision: false,
            tools: false,
            reasoning: api == conversation::ModelApi::DeepSeek,
            web_search: false,
            generates: vec![],
            custom_efforts: false,
            efforts: if api == conversation::ModelApi::DeepSeek {
                vec![Effort::High, Effort::Max, Effort::Low, Effort::Disabled]
            } else {
                vec![]
            },
            default_effort: if api == conversation::ModelApi::DeepSeek {
                Effort::High
            } else {
                Effort::Default
            },
        }],
        options: None,
    };
    execute(
        client,
        Command::PutProvider {
            provider: provider.clone(),
            expected_revision: 0,
        },
    )
    .await;
    format!("{}/fixture", provider.id)
}

#[tokio::test]
async fn completes_before_socket_eof() {
    for remote in [false, true] {
        for api in [
            conversation::ModelApi::Responses,
            conversation::ModelApi::ChatCompletions,
            conversation::ModelApi::DeepSeek,
        ] {
            let server = model_fixture::Server::terminal().await;
            let (directory, node, controller, client, worktree) = fixture(remote).await;
            let context = actions::install_actions(
                &client,
                &directory.path().join("source/package"),
                worktree,
                0,
                &[Action::GenerateText],
            )
            .await;
            let model = configure_api(&client, server.endpoint.clone(), api).await;
            let request = client
                .prepare(Command::GeneratePluginText {
                    effort: None,
                    model,
                    prompt: "Choose a card".into(),
                })
                .with_plugin(context);
            let result =
                tokio::time::timeout(Duration::from_secs(5), client.execute(request.clone()))
                    .await
                    .expect("completion must not wait for socket EOF")
                    .unwrap();
            let Output::PluginText(text) = result else {
                panic!("plugin text expected")
            };
            assert_eq!(text.text, "{\"move\":1}");
            assert_eq!(text.tokens, 16);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            if api == conversation::ModelApi::DeepSeek {
                let sent = server.requests.lock().unwrap()[0].clone();
                assert_eq!(sent["thinking"]["type"], "enabled");
                assert_eq!(sent["reasoning_effort"], "high");
                assert!(sent["max_tokens"].is_null());
                let Output::Providers(providers) = execute(&client, Command::ListProviders).await
                else {
                    panic!("providers expected")
                };
                assert_eq!(providers[0].models[0].default_effort, Effort::High);
                let Command::GeneratePluginText { model, .. } = &request.command else {
                    unreachable!()
                };
                let override_request = client
                    .prepare(Command::GeneratePluginText {
                        model: model.clone(),
                        prompt: "Choose a card".into(),
                        effort: Some(Effort::Max),
                    })
                    .with_plugin(request.plugin.clone().unwrap());
                assert!(matches!(
                    client.execute(override_request).await.unwrap(),
                    Output::PluginText(_)
                ));
                assert_eq!(
                    server.requests.lock().unwrap()[1]["reasoning_effort"],
                    "max"
                );
                let invalid = client
                    .prepare(Command::GeneratePluginText {
                        model: model.clone(),
                        prompt: "Choose a card".into(),
                        effort: Some(Effort::XHigh),
                    })
                    .with_plugin(request.plugin.clone().unwrap());
                assert_eq!(
                    client.execute(invalid).await.unwrap_err().code,
                    ErrorCode::InvalidRequest
                );
                assert_eq!(server.requests.lock().unwrap().len(), 2);
                for (index, effort) in [Effort::Low, Effort::Disabled].into_iter().enumerate() {
                    let next = client
                        .prepare(Command::GeneratePluginText {
                            model: model.clone(),
                            prompt: "Choose a card".into(),
                            effort: Some(effort),
                        })
                        .with_plugin(request.plugin.clone().unwrap());
                    assert!(matches!(
                        client.execute(next).await.unwrap(),
                        Output::PluginText(_)
                    ));
                    let sent = server.requests.lock().unwrap()[index + 2].clone();
                    assert_eq!(
                        sent["thinking"]["type"],
                        if effort == Effort::Disabled {
                            "disabled"
                        } else {
                            "enabled"
                        }
                    );
                    if effort == Effort::Low {
                        assert_eq!(sent["reasoning_effort"], "low");
                    } else {
                        assert!(sent["reasoning_effort"].is_null());
                    }
                }
            }
            if api == conversation::ModelApi::Responses {
                let Command::GeneratePluginText { model, .. } = request.command else {
                    unreachable!()
                };
                let truncated = client
                    .prepare(Command::GeneratePluginText {
                        effort: None,
                        model,
                        prompt: "Truncate the response".into(),
                    })
                    .with_plugin(request.plugin.unwrap());
                let fault = tokio::time::timeout(Duration::from_secs(5), client.execute(truncated))
                    .await
                    .expect("incomplete events must terminate promptly")
                    .unwrap_err();
                assert_eq!(fault.code, ErrorCode::Unavailable);
                assert_eq!(fault.message, "plugin model response was not completed");
                assert_eq!(server.requests.lock().unwrap().len(), 2);
            }
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn recovers_original_result() {
    for remote in [false, true] {
        let server = model_fixture::Server::markdown("{\"move\":1}".into()).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::GenerateText, Action::ReadFiles],
        )
        .await;
        context.worktree = None;
        let model = configure(&client, server.endpoint.clone()).await;
        let request = client
            .prepare(Command::GeneratePluginText {
                effort: None,
                model,
                prompt: "Choose a legal card".into(),
            })
            .with_plugin(context.clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        let Output::PluginText(text) = &result else {
            panic!("plugin text expected")
        };
        assert_eq!(text.text, "{\"move\":1}");
        assert_eq!(text.model, "fixture");
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(result)))
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let sent = server.requests.lock().unwrap()[0].clone();
        assert!(sent["tools"].is_null() || sent["tools"].as_array().unwrap().is_empty());
        assert!(sent["messages"].to_string().contains("Choose a legal card"));
        assert!(!sent.to_string().contains("plugin.json"));

        // A Node-only game does not implicitly acquire project access.
        let read = client
            .prepare(Command::ReadFile {
                worktree,
                path: "notes.txt".into(),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(read).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        let next = client.prepare(request.command.clone()).with_plugin(context);
        assert_eq!(
            client.execute(next).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        // Already admitted results remain recoverable after disabling a package.
        assert!(matches!(
            client.execute(request).await.unwrap(),
            Output::PluginText(_)
        ));
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_requests() {
    let (directory, node, controller, client, worktree) = fixture(false).await;
    let context = actions::install_actions(
        &client,
        &directory.path().join("source/package"),
        worktree,
        0,
        &[Action::ReadFiles],
    )
    .await;
    let command = Command::GeneratePluginText {
        effort: None,
        model: String::new(),
        prompt: "Choose".into(),
    };
    assert_eq!(
        client
            .execute(client.prepare(command.clone()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        client
            .execute(client.prepare(command.clone()).with_plugin(context))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let context = actions::install_actions(
        &client,
        &directory.path().join("source/package"),
        worktree,
        1,
        &[Action::GenerateText],
    )
    .await;
    assert_eq!(
        client
            .execute(client.prepare(command).with_plugin(context.clone()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    for prompt in [String::new(), "x".repeat(65537)] {
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::GeneratePluginText {
                            effort: None,
                            prompt,
                            model: String::new()
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    drop(client);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_failed_attempt() {
    for remote in [false, true] {
        let server = model_fixture::Server::http(1, 503).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::GenerateText],
        )
        .await;
        let model = configure(&client, server.endpoint.clone()).await;
        let request = client
            .prepare(Command::GeneratePluginText {
                effort: None,
                model,
                prompt: "Choose".into(),
            })
            .with_plugin(context);
        let result = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(result.code, ErrorCode::Unavailable);
        assert_eq!(client.execute(request).await.unwrap_err(), result);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn requires_available_model() {
    for remote in [false, true] {
        let first = model_fixture::Server::markdown("first".into()).await;
        let second = model_fixture::Server::markdown("second".into()).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::GenerateText],
        )
        .await;
        configure(&client, first.endpoint.clone()).await;
        let selected = configure(&client, second.endpoint.clone()).await;
        let request = client
            .prepare(Command::ListPluginModels)
            .with_plugin(context.clone());
        let admission = client.dispatch(request).await.unwrap();
        assert!(!admission.receipt.durable);
        let Output::PluginModels(catalog) = admission.completion.await.unwrap().unwrap() else {
            panic!("model catalog expected")
        };
        assert_eq!(catalog.models.len(), 2);
        assert!(catalog.models.iter().any(|model| model.id == selected));
        let public = serde_json::to_string(&catalog).unwrap();
        assert!(!public.contains(&first.endpoint));
        assert!(!public.contains(&second.endpoint));
        assert!(!public.contains("credential"));
        let command = |model| {
            client
                .prepare(Command::GeneratePluginText {
                    effort: None,
                    model,
                    prompt: "Choose".into(),
                })
                .with_plugin(context.clone())
        };
        assert_eq!(
            client
                .execute(command(String::new()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(
            client
                .execute(command(format!(
                    "{}/missing",
                    selected.split_once('/').unwrap().0
                )))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let Output::PluginText(text) = client.execute(command(selected.clone())).await.unwrap()
        else {
            panic!("completion expected")
        };
        assert_eq!(text.text, "second");
        assert!(first.requests.lock().unwrap().is_empty());
        let Output::Providers(providers) = execute(&client, Command::ListProviders).await else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| selected.starts_with(&provider.id.to_string()))
            .unwrap();
        provider.enabled = false;
        execute(
            &client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        assert_eq!(
            client.execute(command(selected)).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert!(first.requests.lock().unwrap().is_empty());
        assert_eq!(second.requests.lock().unwrap().len(), 1);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
