use super::*;
use crate::github_fixture as cli;
use sailry_protocol::plugin::connector::{Input, Kind, LoginState};
use std::sync::atomic::Ordering;
#[path = "../../../plugins/oauth/server.rs"]
mod issuer;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn stored(node: &Node) -> Vec<u8> {
    node.with_storage(|db| {
        Box::pin(async move {
            db.prepare("SELECT body FROM plugin_connections WHERE plugin='http-fixture'")
                .await
                .map_err(crate::database_fixture::fault)?
                .query_row(())
                .await
                .map_err(crate::database_fixture::fault)?
                .get::<Vec<u8>>(0)
                .map_err(crate::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

async fn connect(fixture: &process::Fixture, info: &plugin::Info, method: Kind) {
    let Output::ConnectorLogin(attempt) = fixture
        .client
        .execute(fixture.client.prepare(Command::StartConnectorLogin {
            package: info.summary.reference(),
            connector: "service".into(),
            connection: None,
            expected_revision: None,
            method,
            input: if method == Kind::Oauth2 {
                Input {
                    fields: BTreeMap::from([("client_id".into(), "registered-client".into())]),
                    secrets: BTreeMap::from([(
                        "client_secret".into(),
                        Secret::new("private-client-marker".into()),
                    )]),
                    redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                    ..Default::default()
                }
            } else {
                Input::default()
            },
        }))
        .await
        .unwrap()
    else {
        panic!("connector login expected")
    };
    let mut updates = fixture
        .client
        .subscribe_connector_login(attempt.id)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                panic!("login update expected")
            };
            match update.state {
                LoginState::Browser { url, .. } => {
                    let url = reqwest::Url::parse(&url).unwrap();
                    let query: BTreeMap<_, _> = url.query_pairs().collect();
                    let mut returned = reqwest::Url::parse(&query["redirect_uri"]).unwrap();
                    returned
                        .query_pairs_mut()
                        .append_pair("state", &query["state"])
                        .append_pair("code", "single-code");
                    execute(
                        &fixture.client,
                        Command::SubmitConnectorInput {
                            attempt: attempt.id,
                            input: Secret::new(returned.into()),
                        },
                    )
                    .await;
                }
                LoginState::Connected(_) => break,
                state => assert!(state.active(), "{state:?}"),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn preserves_credentials_after_turn_cancellation() {
    for remote in [false, true] {
        for method in [Kind::Oauth2, Kind::LocalCli] {
            let programs = tempfile::tempdir().unwrap();
            let cli = cli::Cli::new(programs.path());
            let issuer = issuer::Server::start(false, true).await;
            let resource = server::Server::start().await;
            let model =
                Server::tools(vec![(plugin_tool("http-fixture", "request"), json!({}))]).await;
            let fixture = process::Fixture::environment(remote, &model, cli.source()).await;
            fixture
                .node
                .set_connector_fixture("https://service.example.com", &resource.origin);
            fixture
                .node
                .set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            let oauth = json!({"kind":"oauth2","flow":"pkce","authorize":"https://issuer.example.com/authorize","token":"https://issuer.example.com/token","client":{"fields":{"client_id":"client_id","client_secret":"client_secret"}},"scopes":["read"],"refresh":true});
            let github: serde_json::Value =
                serde_json::from_str(include_str!("../../../../../../plugins/github/plugin.json"))
                    .unwrap();
            let mut local =
                github["extensions"]["dev.sailry.platform"]["connectors"][0]["auth"][1].clone();
            local["timeout_ms"] = json!(1000);
            let root = fixture.root.join("package");
            std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
            std::fs::write(root.join("dev.sailry.platform/host/main.js"),
                "export function request({step,outcome}) { if (step === 0) return {call:{operation:'http.request',arguments:{url:'https://service.example.com/ok',method:'GET',connector:'service',timeout_ms:2000}}}; return {result:outcome,output:0}; }"
            ).unwrap();
            std::fs::write(root.join("plugin.json"), json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"http-fixture", "version":"0.1.0",
                "extensions":{"dev.sailry.platform":{
                    "api_version":"v1", "scope":"host", "actions":["http.request"],
                    "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["request"]},
                    "tools":[{"name":"request","description":"Read the fixture resource","handler":{"name":"request","parameters":{"type":"object","properties":{}},"flow":{"operations":["http.request"]}}}],
                    "connectors":[{"id":"service","display":{"label":"Service"},"accounts":"single",
                        "origins":["https://issuer.example.com","https://service.example.com"],
                        "auth":[if method == Kind::Oauth2 { oauth } else { local }],
                        "fields":if method == Kind::Oauth2 { json!([{"name":"client_id","title":"Client ID"},{"name":"client_secret","title":"Client secret","secret":true}]) } else {json!([])},
                        "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
                    }]
                }}
            }).to_string()).unwrap();
            let Output::Plugin(info) = fixture
                .client
                .execute(fixture.client.prepare(Command::InstallPlugin {
                    worktree: fixture.session.worktree,
                    path: "package".into(),
                    name: "http-fixture".into(),
                    expected_revision: 0,
                    expected_digest: None,
                }))
                .await
                .unwrap()
            else {
                panic!("plugin expected")
            };
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            connect(&fixture, &info, method).await;
            tokio::time::sleep(Duration::from_millis(1100)).await;
            let before = stored(&fixture.node).await;
            if method == Kind::Oauth2 {
                issuer.status.store(0, Ordering::SeqCst);
            } else {
                std::fs::write(cli.home().join("gh-hold"), "").unwrap();
            }
            let turn = approvals::submit(&fixture.client, fixture.session.id).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                if method == Kind::Oauth2 {
                    while issuer.bodies.lock().unwrap().len() < 2 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                } else {
                    while !cli.home().join("gh-entered").exists() {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(
                issuer.bodies.lock().unwrap().len(),
                if method == Kind::Oauth2 { 2 } else { 0 }
            );
            assert!(resource.requests.lock().unwrap().is_empty());
            execute(&fixture.client, Command::StopTurn { turn }).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            assert!(!serde_json::to_string(&page).unwrap().contains(cli::TOKEN));
            assert_eq!(stored(&fixture.node).await, before);
            if method == Kind::Oauth2 {
                issuer.status.store(200, Ordering::SeqCst);
            } else {
                std::fs::remove_file(cli.home().join("gh-hold")).unwrap();
            }
            let result = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::RequestPluginHttp(plugin::http::Request {
                            url: "https://service.example.com/ok".into(),
                            method: "GET".into(),
                            headers: BTreeMap::new(),
                            body: None,
                            credential: None,
                            connector: Some("service".into()),
                            connection: None,
                            idempotent: false,
                            timeout_ms: 2000,
                        }))
                        .with_plugin(plugin::Context {
                            package: info.summary.reference(),
                            worktree: Some(fixture.session.worktree),
                            session: None,
                            turn: None,
                            invocation: None,
                            surface: Default::default(),
                        }),
                )
                .await
                .unwrap();
            assert!(matches!(result, Output::PluginHttp(response) if response.status == 201));
            assert_eq!(cli.calls("token") > 0, method == Kind::LocalCli);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
