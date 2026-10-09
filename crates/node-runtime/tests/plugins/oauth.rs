use super::*;
use plugin::oauth::{Attempt, Configuration, Provider, State};
use serde_json::json;
use std::collections::BTreeMap;
#[path = "oauth/server.rs"]
mod server;

fn configure(root: &Path, provider: Provider) {
    let (origin, header, prefix, scope) = match provider {
        Provider::Google => (
            "https://www.googleapis.com",
            "Authorization",
            "Bearer ",
            "https://www.googleapis.com/auth/drive.readonly",
        ),
        Provider::Microsoft => (
            "https://graph.microsoft.com",
            "Authorization",
            "Bearer ",
            "Files.Read",
        ),
        Provider::Dingtalk => (
            "https://api.dingtalk.com",
            "x-acs-dingtalk-access-token",
            "",
            "openid",
        ),
    };
    let manifest = root.join("plugin.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["extensions"] = json!({"dev.sailry.platform": {
        "api_version":"v1", "scope":"host", "actions":["http.request"],
        "settings_schema":"dev.sailry.platform/settings.json",
        "oauth":{"provider":provider,"scopes":[scope],"credentials":["token"]}
    }});
    fs::write(manifest, value.to_string()).unwrap();
    fs::write(root.join("dev.sailry.platform/settings.json"), json!({
        "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{"token":{"type":"string","x-sailry-secret":{"origin":origin,"header":header,"prefix":prefix}}},"required":[]
    }).to_string()).unwrap();
}

async fn state(client: &Client, attempt: &Attempt, ready: impl Fn(&State) -> bool) -> State {
    let mut subscription = client.subscribe_plugin_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Update::PluginLogin(update) = subscription.next().await.unwrap() else {
                panic!("plugin login update expected");
            };
            let public = serde_json::to_string(&update).unwrap();
            for secret in [
                "private-access-marker",
                "private-refresh-marker",
                "private-client-marker",
            ] {
                assert!(!public.contains(secret));
            }
            if ready(&update.state) {
                return update.state;
            }
            assert!(
                update.state.active(),
                "unexpected login state: {:?}",
                update.state
            );
        }
    })
    .await
    .expect("plugin login deadline")
}

async fn begin(
    client: &Client,
    package: plugin::Reference,
    provider: Provider,
) -> (Request, Attempt, String) {
    let request = client.prepare(Command::BeginPluginLogin {
        package,
        redirect: "http://127.0.0.1:43191/callback".into(),
        configuration: Configuration {
            client_id: "registered-client".into(),
            tenant: None,
            client_secret: (provider != Provider::Microsoft)
                .then(|| Secret::new("private-client-marker".into())),
        },
    });
    let Output::PluginLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
        panic!("login attempt expected");
    };
    let State::Pending { url, .. } = state(client, &attempt, |state| {
        matches!(state, State::Pending { .. })
    })
    .await
    else {
        unreachable!();
    };
    (request, attempt, url)
}

fn callback(url: &str) -> Secret {
    let authorization = reqwest::Url::parse(url).unwrap();
    let fields: BTreeMap<_, _> = authorization.query_pairs().collect();
    let mut callback = reqwest::Url::parse(fields["redirect_uri"].as_ref()).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &fields["state"])
        .append_pair("code", "single-code");
    Secret::new(callback.into())
}

#[tokio::test]
async fn binds_account_login_and_revocation() {
    for remote in [false, true] {
        for provider in [Provider::Google, Provider::Microsoft, Provider::Dingtalk] {
            let server = server::Server::start(provider == Provider::Dingtalk, false).await;
            let (directory, node, controller, client, worktree) = fixture(remote).await;
            node.set_plugin_oauth_fixture(provider, &server.endpoint);
            configure(&directory.path().join("source/package"), provider);
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let (request, attempt, url) = begin(&client, package.clone(), provider).await;
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::PluginLogin(attempt.clone())
            );
            let other = Client::new(if remote {
                node.local()
            } else {
                controller.handle().remote(node.link().address())
            });
            let denied = match other.subscribe_plugin_login(attempt.id).await {
                Ok(mut subscription) => subscription.next().await.unwrap_err(),
                Err(error) => error,
            };
            assert_eq!(denied.code, ErrorCode::NotFound);
            assert_eq!(
                other
                    .execute(other.prepare(Command::CompletePluginLogin {
                        attempt: attempt.id,
                        callback: callback(&url)
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let complete = client.prepare(Command::CompletePluginLogin {
                attempt: attempt.id,
                callback: callback(&url),
            });
            assert_eq!(
                client.execute(complete.clone()).await.unwrap(),
                Output::PluginLogin(attempt.clone())
            );
            let State::Connected(package) = state(&client, &attempt, |state| {
                matches!(state, State::Connected(_))
            })
            .await
            else {
                unreachable!();
            };
            assert_eq!(
                client.execute(complete).await.unwrap(),
                Output::PluginLogin(attempt)
            );
            assert_eq!(server.bodies.lock().unwrap().len(), 1);
            let body = server.bodies.lock().unwrap()[0].clone();
            if provider == Provider::Dingtalk {
                let body: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(body["clientId"], "registered-client");
                assert_eq!(body["code"], "single-code");
                assert_eq!(body["clientSecret"], "private-client-marker");
                assert_eq!(body["grantType"], "authorization_code");
            } else {
                let fields: BTreeMap<_, _> =
                    reqwest::Url::parse(&format!("http://fixture/?{body}"))
                        .unwrap()
                        .query_pairs()
                        .map(|(key, value)| (key.into_owned(), value.into_owned()))
                        .collect();
                assert_eq!(fields["grant_type"], "authorization_code");
                assert_eq!(fields["code"], "single-code");
                assert!(fields["code_verifier"].len() >= 43);
            }
            let Output::PluginSecret(secret) = execute(
                &client,
                Command::ReadPluginSecret {
                    package: package.clone(),
                    field: "token".into(),
                },
            )
            .await
            else {
                panic!("controller secret expected");
            };
            assert_eq!(secret.unwrap().expose(), "private-access-marker");
            let Output::PluginAuthorization(status) =
                execute(&client, Command::RevokePluginAuthorization { package }).await
            else {
                panic!("account status expected");
            };
            assert!(!status.configured);
            let Output::PluginSecret(secret) = execute(
                &client,
                Command::ReadPluginSecret {
                    package: status.package,
                    field: "token".into(),
                },
            )
            .await
            else {
                panic!("controller secret expected");
            };
            assert!(secret.is_none());
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_stale_and_forged_callbacks() {
    for remote in [false, true] {
        let server = server::Server::start(false, false).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        node.set_plugin_oauth_fixture(Provider::Google, &server.endpoint);
        configure(&directory.path().join("source/package"), Provider::Google);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let (_, attempt, url) = begin(&client, package.clone(), Provider::Google).await;
        let mut forged = reqwest::Url::parse(callback(&url).expose()).unwrap();
        forged
            .query_pairs_mut()
            .append_pair("state", "wrong-attempt");
        execute(
            &client,
            Command::CompletePluginLogin {
                attempt: attempt.id,
                callback: Secret::new(forged.into()),
            },
        )
        .await;
        assert!(matches!(
            state(&client, &attempt, |state| matches!(state, State::Failed(_))).await,
            State::Failed(_)
        ));
        assert!(server.bodies.lock().unwrap().is_empty());
        let (_, attempt, url) = begin(&client, package.clone(), Provider::Google).await;
        let Output::PluginSettings(saved) = execute(
            &client,
            Command::SavePluginSettings {
                package,
                values: BTreeMap::new(),
                secrets: BTreeMap::from([("token".into(), plugin::settings::SecretUpdate::Clear)]),
            },
        )
        .await
        else {
            panic!("saved settings expected");
        };
        execute(
            &client,
            Command::CompletePluginLogin {
                attempt: attempt.id,
                callback: callback(&url),
            },
        )
        .await;
        assert!(matches!(
            state(&client, &attempt, |state| matches!(state, State::Failed(_))).await,
            State::Failed(Fault {
                code: ErrorCode::RevisionConflict,
                ..
            })
        ));
        let Output::PluginAuthorization(status) = execute(
            &client,
            Command::ReadPluginAuthorization {
                package: saved.package.clone(),
            },
        )
        .await
        else {
            panic!("account status expected");
        };
        assert!(!status.configured);
        let (_, attempt, url) = begin(&client, saved.package, Provider::Google).await;
        execute(
            &client,
            Command::CancelPluginLogin {
                attempt: attempt.id,
            },
        )
        .await;
        assert_eq!(
            state(&client, &attempt, |state| *state == State::Cancelled).await,
            State::Cancelled
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::CompletePluginLogin {
                    attempt: attempt.id,
                    callback: callback(&url)
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(server.bodies.lock().unwrap().len(), 1);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
