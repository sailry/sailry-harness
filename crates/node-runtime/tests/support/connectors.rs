//! Shared current-contract fixtures for connection service tests.
use sailry_client::Client;
use sailry_protocol::{
    Command, Output, Request, Secret, Update,
    plugin::{self, connector::*},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
pub const ORIGIN: &str = "https://service.example.com";
pub fn application(server: bool) -> Value {
    json!({"id":"service","display":{"label":"Service"},"accounts":"single","origins":[ORIGIN],
    "auth":[{"kind":"application","fields":{"client_id":"app_id","client_secret":"app_secret"},"token":{"url":format!("{ORIGIN}/token"),"body":{"app_id":"{client_id}","app_secret":"{client_secret}"},"access":"/access_token","expires_in":"/expires_in"}}],
    "fields":[{"name":"app_id","title":"App ID"},{"name":"app_secret","title":"App secret","secret":true}],
    "inject":[if server {json!({"server":"input","header":"X-Access-Token"})}else{json!({"origin":ORIGIN,"header":"X-Access-Token"})}]})
}
pub fn configure(root: &std::path::Path, declaration: Value) {
    let filename = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&filename).unwrap()).unwrap();
    let extension = &mut manifest["extensions"]["dev.sailry.platform"];
    if !extension.is_object() {
        *extension = json!({"api_version":"v1","scope":"host"});
    }
    if declaration["inject"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| target.get("origin").is_some())
    {
        extension["actions"] = json!(["http.request"]);
    }
    extension["connectors"] = json!([declaration]);
    std::fs::write(filename, manifest.to_string()).unwrap();
}
pub fn start(
    client: &Client,
    package: plugin::Reference,
    method: Kind,
    registration: bool,
) -> Request {
    client.prepare(Command::StartConnectorLogin {
        package,
        connector: "service".into(),
        connection: None,
        expected_revision: None,
        method,
        input: Input {
            fields: BTreeMap::from([("app_id".into(), "fixture-app".into())]),
            secrets: BTreeMap::from([(
                "app_secret".into(),
                Secret::new("application-private-marker".into()),
            )]),
            registration,
            ..Default::default()
        },
    })
}
pub async fn state(
    client: &Client,
    attempt: &Attempt,
    ready: impl Fn(&LoginState) -> bool,
) -> LoginState {
    let mut stream = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                panic!("connector login expected")
            };
            for marker in [
                "application-private-marker",
                "private-access-marker",
                "private-refresh-marker",
                "private-client-marker",
            ] {
                assert!(!serde_json::to_string(&update).unwrap().contains(marker));
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
    .expect("connector login deadline")
}
pub async fn connected(client: &Client, request: Request) -> Connection {
    let Output::ConnectorLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
        panic!("connector login expected")
    };
    assert_eq!(
        client.execute(request).await.unwrap(),
        Output::ConnectorLogin(attempt.clone())
    );
    let LoginState::Connected(connection) = state(client, &attempt, |state| {
        matches!(state, LoginState::Connected(_))
    })
    .await
    else {
        unreachable!()
    };
    connection
}
