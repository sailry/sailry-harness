//! Persisted account input is migrated without turning form defaults into accounts.
use super::*;
use sailry_protocol::plugin::{
    Context,
    connector::{Kind, State},
    http,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

fn backups(profile: &Path) -> Vec<PathBuf> {
    let path = profile.join("storage/backups");
    if !path.exists() {
        return vec![];
    }
    fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect()
}

async fn seed(profile: &Path, info: &mut Info, saved: &Value) -> Vec<u8> {
    let (db_handle, db) =
        crate::plugins::database_fixture::open(&(profile.join("storage/node.turso")), false).await;
    info.summary.version = Some("0.0.1".into());
    info.summary.settings_revision = 1;
    info.origin = Some(plugin::Origin::Bundled);
    info.extension.as_mut().unwrap().connectors.clear();
    let bytes = serde_json::to_vec_pretty(saved).unwrap();
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)",
        turso::params![info.summary.name.as_str(), bytes.as_slice()],
    )
    .await
    .unwrap();
    db.execute(
        "UPDATE plugins SET settings_revision=1,body=?2 WHERE name=?1",
        turso::params![
            info.summary.name.as_str(),
            serde_json::to_vec(info).unwrap()
        ],
    )
    .await
    .unwrap();
    crate::plugins::database_fixture::close(db_handle, db).await;
    bytes
}

async fn retained(db: &turso::Connection, name: &str) -> turso::Result<Vec<u8>> {
    db.prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=1")
        .await?
        .query_row([name])
        .await?
        .get(0)
}

#[tokio::test]
async fn preserves_credentials_and_microsoft_tenant() {
    for remote in [false, true] {
        for name in ["microsoft-365", "feishu"] {
            let resource = super::super::http::server::Server::respond(Some(json!({
                "access_token":"private-microsoft-rotated","refresh_token":"private-microsoft-refresh-rotated",
                "expires_in":3600,"token_type":"Bearer",
                "tenant_access_token":"private-feishu-access","expire":7200
            }).to_string()), None).await;
            let (_directory, node, controller, client, worktree) = support::empty(remote).await;
            let mut info = info(
                execute(
                    &client,
                    Command::InstallBundledPlugin {
                        name: name.into(),
                        expected_revision: 0,
                    },
                )
                .await,
            );
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            assert!(rows.is_empty());
            let profile = node.profile().to_owned();
            let id = node.id();
            node.shutdown().await.unwrap();
            controller.handle().disconnect(id).await;
            let saved = if name == "microsoft-365" {
                json!({"schema":null,"values":{"unrelated":"retained"},"slots":{},
                "oauth":{"id":CredentialId::new(),"revision":3,"grant":{
                    "declaration":{"provider":"microsoft","scopes":["Mail.Read"],"credentials":["token"]},
                    "configuration":{"client_id":"retained-client","client_secret":null,"tenant":"retained-tenant"},
                    "access_token":"private-microsoft-expired","refresh_token":"private-microsoft-refresh","expires_at_ms":0
                }}})
            } else {
                json!({"schema":null,"values":{"app_id":"retained-app","unrelated":"retained"},
                    "slots":{"app_secret":"private-feishu-secret"}})
            };
            let original = seed(&profile, &mut info, &saved).await;
            let before = backups(&profile);
            let node = Node::start_empty(&profile).await.unwrap();
            for origin in [
                "https://graph.microsoft.com",
                "https://login.microsoftonline.com",
                "https://open.feishu.cn",
            ] {
                node.set_connector_fixture(origin, &resource.origin);
            }
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            execute(
                &client,
                Command::InstallBundledPlugin {
                    name: name.into(),
                    expected_revision: info.summary.revision,
                },
            )
            .await;
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            let row = rows.iter().find(|row| row.package.name == name).unwrap();
            assert_eq!(row.state, State::Connected);
            if name == "microsoft-365" {
                assert_eq!(row.method, Kind::Oauth2);
                assert_eq!(row.fields["tenant"], "retained-tenant");
                assert_eq!(row.fields["client_id"], "retained-client");
            } else {
                assert_eq!(row.method, Kind::Application);
                assert_eq!(row.fields["app_id"], "retained-app");
                assert!(row.configured.contains(&"app_secret".into()));
            }
            let response = if name == "microsoft-365" {
                let response = client
                    .execute(
                        client
                            .prepare(Command::RequestPluginHttp(http::Request {
                                url: "https://graph.microsoft.com/public".into(),
                                method: "GET".into(),
                                headers: BTreeMap::new(),
                                body: None,
                                credential: None,
                                connector: Some(row.connector.clone()),
                                connection: Some(row.id),
                                idempotent: false,
                                timeout_ms: 2000,
                            }))
                            .with_plugin(Context {
                                package: row.package.clone(),
                                worktree: Some(worktree),
                                session: None,
                                turn: None,
                                invocation: None,
                                surface: Default::default(),
                            }),
                    )
                    .await
                    .unwrap();
                assert!(
                    matches!(response, Output::PluginHttp(ref response) if response.status == 200)
                );
                response
            } else {
                let response =
                    execute(&client, Command::ProbeConnection { connection: row.id }).await;
                assert!(
                    matches!(response, Output::Connection(ref row) if row.state == State::Connected)
                );
                response
            };
            {
                let requests = resource.requests.lock().unwrap();
                assert_eq!(requests.len(), if name == "microsoft-365" { 2 } else { 1 });
                if name == "microsoft-365" {
                    assert!(
                        requests[0]
                            .0
                            .starts_with("POST /retained-tenant/oauth2/v2.0/token ")
                    );
                    let fields: BTreeMap<_, _> =
                        url::form_urlencoded::parse(requests[0].1.as_bytes()).collect();
                    assert_eq!(fields["client_id"], "retained-client");
                    assert_eq!(fields["grant_type"], "refresh_token");
                    assert_eq!(fields["refresh_token"], "private-microsoft-refresh");
                    assert!(!fields.contains_key("client_secret"));
                    assert!(
                        requests[1]
                            .0
                            .contains("authorization: Bearer private-microsoft-rotated\r\n")
                    );
                } else {
                    assert!(
                        requests[0]
                            .0
                            .starts_with("POST /open-apis/auth/v3/tenant_access_token/internal ")
                    );
                    let fields: Value = serde_json::from_str(&requests[0].1).unwrap();
                    assert_eq!(fields["app_id"], "retained-app");
                    assert_eq!(fields["app_secret"], "private-feishu-secret");
                }
            }

            assert_eq!(
                {
                    let package = name.to_owned();
                    node.with_storage(move |db| {
                        Box::pin(async move {
                            retained(db, &package)
                                .await
                                .map_err(crate::plugins::database_fixture::fault)
                        })
                    })
                    .await
                    .unwrap()
                },
                original
            );
            if name == "feishu" {
                let body: Vec<u8> = {
                    let parameters = turso::params![row.id.to_string()];
                    let sql = "SELECT body FROM plugin_connections WHERE id=?1";
                    node.with_storage(move |db| {
                        Box::pin(async move {
                            let row = db
                                .prepare(sql)
                                .await
                                .map_err(crate::plugins::database_fixture::fault)?
                                .query_row(parameters)
                                .await
                                .map_err(crate::plugins::database_fixture::fault)?;
                            let result: turso::Result<_> = row.get(0);
                            result.map_err(crate::plugins::database_fixture::fault)
                        })
                    })
                    .await
                    .unwrap()
                };
                let saved: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(saved["grant"]["access"], "private-feishu-access");
            }

            let after = backups(&profile);
            assert_eq!(after.len(), before.len() + 1);
            let (backup_handle, backup) = crate::plugins::database_fixture::open(
                after.iter().find(|path| !before.contains(path)).unwrap(),
                true,
            )
            .await;
            assert_eq!(retained(&backup, name).await.unwrap(), original);
            crate::plugins::database_fixture::close(backup_handle, backup).await;
            let public = serde_json::to_string(&(rows, response)).unwrap();
            for secret in ["private-microsoft-", "private-feishu-"] {
                assert!(!public.contains(secret));
            }
            node.shutdown().await.unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            assert_eq!(backups(&profile), after);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
