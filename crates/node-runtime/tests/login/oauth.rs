use super::*;
use sailry_protocol::conversation::oauth::Options;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn options(authentication: Authentication, suffix: &str) -> Options {
    match authentication {
        Authentication::ChatGpt => Options::ChatGpt {
            catalog_version: "0.161.0".into(),
            user_agent: format!("Sailry-Fixture/{suffix}"),
        },
        Authentication::Copilot => Options::Copilot {
            user_agent: format!("Sailry-Fixture/{suffix}"),
            editor_version: format!("fixture-editor/{suffix}"),
            editor_plugin_version: format!("fixture-plugin/{suffix}"),
        },
        _ => unreachable!(),
    }
}

pub(super) fn headers(request: &discovery_support::Request, options: &Options) {
    assert_eq!(request.headers["user-agent"], options.user_agent());
    if let Options::Copilot {
        editor_version,
        editor_plugin_version,
        ..
    } = options
    {
        assert_eq!(request.headers["editor-version"], *editor_version);
        assert_eq!(
            request.headers["editor-plugin-version"],
            *editor_plugin_version
        );
    }
}

fn command(provider: &Provider, options: Option<Options>) -> Command {
    Command::DiscoverModels(Box::new(discovery::Source::Saved {
        provider: provider.id,
        expected_revision: provider.revision,
        oauth: options,
    }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovers_with_unsaved_options() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.starts_with("/models") {
                    Reply::Json(catalog::models(authentication))
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = execution::signed_in(&fixture, authentication, api).await;
            let draft = options(authentication, "draft");
            let Output::DiscoveredModels(models) = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command(&provider, Some(draft.clone()))),
                )
                .await
                .unwrap()
            else {
                panic!("catalog expected")
            };
            assert_eq!(models.models.len(), 2);
            let Output::Providers(saved) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            assert_eq!(
                saved.iter().find(|value| value.id == provider.id),
                Some(&provider)
            );
            {
                let requests = server.requests.lock().unwrap();
                let request = requests.last().unwrap();
                headers(request, &draft);
                if authentication == Authentication::ChatGpt {
                    assert_eq!(request.path, "/models?client_version=0.161.0");
                    assert_eq!(request.headers["originator"], "sailry");
                }
            }
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_invalid_options_without_http_or_writes() {
    for remote in [false, true] {
        for authentication in [Authentication::ChatGpt, Authentication::Copilot] {
            let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = provider(&fixture.client, authentication).await;
            let mut invalid = options(authentication, "invalid");
            match &mut invalid {
                Options::ChatGpt {
                    catalog_version, ..
                } => *catalog_version = "v0.161".into(),
                Options::Copilot { editor_version, .. } => {
                    *editor_version = "editor\r\nx-secret: injected".into()
                }
            }
            let mismatched = options(
                if authentication == Authentication::ChatGpt {
                    Authentication::Copilot
                } else {
                    Authentication::ChatGpt
                },
                "wrong",
            );
            for options in [invalid, mismatched] {
                let mut changed = provider.clone();
                changed.oauth = Some(options.clone());
                for command in [
                    Command::PutProvider {
                        provider: changed.clone(),
                        expected_revision: provider.revision,
                    },
                    command(&provider, Some(options)),
                ] {
                    let error = fixture
                        .client
                        .execute(fixture.client.prepare(command))
                        .await
                        .unwrap_err();
                    assert_eq!(error.code, ErrorCode::InvalidRequest);
                    no_secrets(&error);
                }
            }
            assert!(server.requests.lock().unwrap().is_empty());
            let Output::Providers(saved) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            assert_eq!(
                saved.iter().find(|value| value.id == provider.id),
                Some(&provider)
            );
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalidates_late_draft_after_revision_change() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let queries = AtomicUsize::new(0);
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.starts_with("/models") {
                    if queries.fetch_add(1, Ordering::SeqCst) > 0 {
                        Reply::Delayed(gate.clone(), catalog::models(authentication))
                    } else {
                        Reply::Json(catalog::models(authentication))
                    }
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = execution::signed_in(&fixture, authentication, api).await;
            let client = fixture.client.clone();
            let request =
                client.prepare(command(&provider, Some(options(authentication, "draft"))));
            let query = tokio::spawn(async move { client.execute(request).await });
            server.wait_requests(5).await;
            let mut changed = provider.clone();
            changed.oauth = Some(options(authentication, "saved"));
            let Output::Provider(saved) = fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider: changed,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            release.notify_one();
            let error = query.await.unwrap().unwrap_err();
            assert_eq!(error.code, ErrorCode::RevisionConflict);
            let Output::Providers(providers) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            assert_eq!(
                providers.iter().find(|value| value.id == provider.id),
                Some(&saved)
            );
            fixture.close().await;
        }
    }
}
