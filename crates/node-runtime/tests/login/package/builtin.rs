use super::super::codex;
use super::*;

async fn directory(client: &Client) -> presets::Listing {
    let Output::ChannelPresets(listing) = support::execute(
        client,
        Command::ListChannelPresets {
            refresh_partners: false,
        },
    )
    .await
    else {
        panic!("channel directory expected");
    };
    no_secrets(&listing);
    listing
}

async fn binding(fixture: &Fixture, provider: &Provider) -> serde_json::Value {
    let id = provider.id.to_string();
    let path = format!(
        "$.authorizations.\"{}\".auth",
        provider.credential.as_ref().unwrap().id
    );
    let body: String = fixture
        .node
        .with_storage(move |db| {
            Box::pin(async move {
                let row = db
                    .prepare("SELECT json_extract(body,?2) FROM providers WHERE id=?1")
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .query_row(turso::params![id, path])
                    .await
                    .map_err(crate::database_fixture::fault)?;
                row.get(0).map_err(crate::database_fixture::fault)
            })
        })
        .await
        .unwrap();
    serde_json::from_str(&body).unwrap()
}

async fn rejected(client: &Client, provider: &Provider, code: ErrorCode) {
    let error = client
        .execute(client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
            redirect: None,
        }))
        .await
        .unwrap_err();
    no_secrets(&error);
    assert_eq!(error.code, code);
}

async fn duplicate(fixture: &Fixture) -> plugin::Info {
    let source = fixture.directory.path().join("source");
    let root = source.join("package");
    support::package(&root, "1.0.0");
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1","actions":["models.authenticate","credentials.inspect"],
        "model_auth":[{"id":"duplicate","authentication":"chat_gpt_oauth",
            "display":{"label":"Duplicate sign-in"},
            "flow":{"kind":"script","script":{"entry":"dev.sailry.platform/auth.js"},
                "handler":"authorize","requests":{"token":{"url":"https://issuer.example.test/token"}},
                "callback":null,"issuer":null}}]}});
    std::fs::write(path, manifest.to_string()).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/auth.js"),
        "export function authorize() { throw Error('Ambiguous alias executed'); }",
    )
    .unwrap();
    support::execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Authentication alias fixture".into(),
            path: source.to_str().unwrap().into(),
        },
    )
    .await;
    let Output::Snapshot(snapshot) = support::execute(&fixture.client, Command::Snapshot).await
    else {
        panic!("snapshot expected");
    };
    let installed = support::info(
        support::execute(
            &fixture.client,
            support::install(snapshot.worktrees[0].id, 0),
        )
        .await,
    );
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    assert!(installed.summary.available());
    installed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_accounts_during_alias_changes() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            codex::server::standard(request, 3600)
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let installed = support::info(
            support::execute(
                &fixture.client,
                Command::ReadPlugin {
                    name: "openai-auth".into(),
                },
            )
            .await,
        );
        let listing = directory(&fixture.client).await;
        let entry = listing
            .entries
            .iter()
            .find(|entry| {
                entry.source == presets::Source::Builtin && entry.preset.id == "chat-gpt-oauth"
            })
            .unwrap();
        assert_eq!(
            entry.preset.auth,
            presets::Auth::Builtin {
                authentication: Authentication::ChatGptOAuth
            }
        );
        let declaration = installed
            .extension
            .as_ref()
            .unwrap()
            .model_auth
            .iter()
            .find(|item| item.authentication == Some(Authentication::ChatGptOAuth))
            .unwrap();
        assert_eq!(entry.login, Some(declaration.flow.presentation()));
        let mut provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
        provider.preset = Some(presets::PresetRef {
            source: entry.source.clone(),
            id: entry.preset.id.clone(),
            route: Some(entry.preset.routes[0].key()),
        });
        let Output::Provider(provider) = support::execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected");
        };
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        drop(stream);
        let provider = saved(&fixture.client, provider.id).await;
        let accounts = credentials(&fixture.client).await;
        no_secrets(&provider);
        no_secrets(&accounts);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, provider.credential.as_ref().unwrap().id);
        assert_eq!(
            accounts[0].account_label.as_deref(),
            Some("codex@example.com")
        );
        let target = binding(&fixture, &provider).await;
        assert_eq!(target["package"], installed.summary.name);
        assert_eq!(target["flow"], declaration.id);
        assert_eq!(target["digest"], installed.summary.digest);
        assert_eq!(target["provider"], provider.id.to_string());
        assert_eq!(
            target["origin"],
            serde_json::to_value(consent::Origin::Bundled).unwrap()
        );
        let requests = server.requests.lock().unwrap().len();
        let disabled = support::info(
            support::execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: installed.summary.name.clone(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let mut unavailable = listing.clone();
        for entry in &mut unavailable.entries {
            if matches!(
                entry.preset.auth,
                presets::Auth::Builtin {
                    authentication: Authentication::ChatGpt | Authentication::ChatGptOAuth
                }
            ) {
                entry.login = None;
            }
        }
        assert_eq!(directory(&fixture.client).await, unavailable);
        rejected(&fixture.client, &provider, ErrorCode::NotConfigured).await;
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        assert_eq!(credentials(&fixture.client).await, accounts);
        let enabled = support::info(
            support::execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: disabled.summary.name,
                    expected_revision: disabled.summary.revision,
                    enabled: true,
                },
            )
            .await,
        );
        assert_eq!(directory(&fixture.client).await, listing);
        let duplicate = duplicate(&fixture).await;
        let conflict = fixture
            .client
            .execute(fixture.client.prepare(Command::ListChannelPresets {
                refresh_partners: false,
            }))
            .await
            .unwrap_err();
        no_secrets(&conflict);
        assert_eq!(conflict.code, ErrorCode::Conflict);
        rejected(&fixture.client, &provider, ErrorCode::Conflict).await;
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        assert_eq!(credentials(&fixture.client).await, accounts);
        support::execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: duplicate.summary.name,
                expected_revision: duplicate.summary.revision,
                enabled: false,
            },
        )
        .await;
        assert_eq!(directory(&fixture.client).await, listing);
        support::execute(
            &fixture.client,
            Command::RemovePlugin {
                name: enabled.summary.name,
                expected_revision: enabled.summary.revision,
            },
        )
        .await;
        assert_eq!(directory(&fixture.client).await, unavailable);
        rejected(&fixture.client, &provider, ErrorCode::NotConfigured).await;
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        assert_eq!(credentials(&fixture.client).await, accounts);
        assert_eq!(binding(&fixture, &provider).await, target);
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        fixture.close().await;
    }
}
