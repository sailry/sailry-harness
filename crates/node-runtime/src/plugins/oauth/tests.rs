use super::*;
use sailry_protocol::{
    Secret,
    plugin::oauth::{Configuration, Provider, State},
};
#[path = "../../../tests/plugins/oauth/server.rs"]
mod issuer;

#[tokio::test]
async fn serializes_refresh_and_obeys_revocation() {
    use sailry_client::Client;
    use sailry_link::CancellationToken;
    use sailry_protocol::{Command, Output};
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_with_plugin_catalog(
        directory.path().join("node"),
        "http://127.0.0.1:9/",
    )
    .await
    .unwrap();
    let issuer = issuer::Server::start(false, true).await;
    node.set_plugin_oauth_fixture(Provider::Google, &issuer.endpoint);
    let client = Client::new(node.local());
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::InstallBundledPlugin {
            name: "google-workspace".into(),
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin expected");
    };
    let declaration = info
        .extension
        .as_ref()
        .unwrap()
        .oauth
        .as_ref()
        .unwrap()
        .clone();
    let Output::PluginLogin(attempt) = client
        .execute(client.prepare(Command::BeginPluginLogin {
            package: info.summary.reference(),
            redirect: "http://127.0.0.1:43321/callback".into(),
            configuration: configuration(Provider::Google),
        }))
        .await
        .unwrap()
    else {
        panic!("attempt expected");
    };
    let mut stream = client.subscribe_plugin_login(attempt.id).await.unwrap();
    let url = loop {
        let sailry_protocol::Update::PluginLogin(update) =
            tokio::time::timeout(std::time::Duration::from_secs(15), stream.next())
                .await
                .expect("account progress deadline")
                .unwrap()
        else {
            unreachable!();
        };
        if let State::Pending { url, .. } = update.state {
            break url;
        }
    };
    let authorization = reqwest::Url::parse(&url).unwrap();
    let fields: std::collections::BTreeMap<_, _> = authorization.query_pairs().collect();
    let mut callback = reqwest::Url::parse("http://127.0.0.1:43321/callback").unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &fields["state"])
        .append_pair("code", "single-code");
    client
        .execute(client.prepare(Command::CompletePluginLogin {
            attempt: attempt.id,
            callback: Secret::new(callback.into()),
        }))
        .await
        .unwrap();
    let package = loop {
        let sailry_protocol::Update::PluginLogin(update) =
            tokio::time::timeout(std::time::Duration::from_secs(15), stream.next())
                .await
                .expect("account progress deadline")
                .unwrap()
        else {
            unreachable!();
        };
        if let State::Connected(package) = update.state {
            break package;
        }
        assert!(update.state.active());
    };
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
            rusqlite::params![package.name, package.settings_revision as i64],
            |row| row.get(0),
        )
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = serde_json::from_value(saved["oauth"]["id"].clone()).unwrap();
    drop(db);
    let target = Target {
        plugin: package.name.clone(),
        id,
        declaration,
        header: reqwest::header::AUTHORIZATION,
        prefix: "Bearer ".into(),
    };
    let stop = CancellationToken::new();
    let (first, second) = tokio::join!(
        node.ingress.oauth.access(&node.ingress, &target, &stop),
        node.ingress.oauth.access(&node.ingress, &target, &stop)
    );
    assert_eq!(
        first.unwrap().to_str().unwrap(),
        "Bearer refreshed-access-marker"
    );
    assert_eq!(
        second.unwrap().to_str().unwrap(),
        "Bearer refreshed-access-marker"
    );
    assert_eq!(
        issuer.bodies.lock().unwrap().len(),
        2,
        "one login and one serialized refresh"
    );
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::ReadPlugin {
            name: package.name.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("plugin expected");
    };
    assert_eq!(
        info.summary.reference(),
        package,
        "token rotation preserves the admitted configuration"
    );
    client
        .execute(client.prepare(Command::RevokePluginAuthorization { package }))
        .await
        .unwrap();
    assert!(
        node.ingress
            .oauth
            .access(&node.ingress, &target, &stop)
            .await
            .is_err()
    );
    assert_eq!(issuer.bodies.lock().unwrap().len(), 2);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn exchanges_and_refreshes_fixed_accounts() {
    for provider in [Provider::Google, Provider::Microsoft, Provider::Dingtalk] {
        let issuer = issuer::Server::start(provider == Provider::Dingtalk, false).await;
        let service = Service::default();
        service.fixture(provider, &issuer.endpoint);
        let flow = Flow::begin(
            declaration(provider),
            configuration(provider),
            "http://127.0.0.1:43321/callback",
        )
        .unwrap();
        let authorization = reqwest::Url::parse(flow.url()).unwrap();
        let fields: std::collections::BTreeMap<_, _> = authorization.query_pairs().collect();
        let mut callback = reqwest::Url::parse("http://127.0.0.1:43321/callback").unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", &fields["state"])
            .append_pair("code", "single-code");
        let grant = flow
            .complete(&service, &Secret::new(callback.into()))
            .await
            .unwrap();
        assert_eq!(grant.access_token.expose(), "private-access-marker");
        assert!(grant.expires_at_ms > flow::now().unwrap());
        let renewed = service
            .exchange(
                &grant.declaration,
                &grant.configuration,
                grant.refresh_token.as_ref().unwrap(),
                None,
                Some(&grant),
            )
            .await
            .unwrap();
        assert_eq!(
            renewed.refresh_token.as_ref().unwrap().expose(),
            "private-refresh-marker"
        );
        let bodies = issuer.bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        if provider == Provider::Dingtalk {
            let body: serde_json::Value = serde_json::from_str(&bodies[1]).unwrap();
            assert_eq!(body["grantType"], "refresh_token");
            assert_eq!(body["refreshToken"], "private-refresh-marker");
            assert!(body.get("code").is_none());
        } else {
            let values: std::collections::BTreeMap<_, _> =
                reqwest::Url::parse(&format!("http://fixture/?{}", bodies[1]))
                    .unwrap()
                    .query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect();
            assert_eq!(values["grant_type"], "refresh_token");
            assert_eq!(values["refresh_token"], "private-refresh-marker");
            assert!(!values.contains_key("code"));
        }
    }
}

fn declaration(provider: Provider) -> Declaration {
    Declaration {
        provider,
        scopes: vec![
            match provider {
                Provider::Google => "https://www.googleapis.com/auth/drive.readonly",
                Provider::Microsoft => "Files.Read",
                Provider::Dingtalk => "openid",
            }
            .into(),
        ],
        credentials: vec!["token".into()],
    }
}

fn configuration(provider: Provider) -> Configuration {
    Configuration {
        client_id: "registered-client".into(),
        client_secret: (provider == Provider::Dingtalk)
            .then(|| Secret::new("private-client-secret".into())),
        tenant: None,
    }
}

#[test]
fn binds_callbacks_to_exact_attempt() {
    for provider in [Provider::Google, Provider::Microsoft, Provider::Dingtalk] {
        let flow = Flow::begin(
            declaration(provider),
            configuration(provider),
            "http://127.0.0.1:43321/callback",
        )
        .unwrap();
        let authorization = reqwest::Url::parse(flow.url()).unwrap();
        let fields: std::collections::BTreeMap<_, _> = authorization
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(fields["response_type"], "code");
        assert_eq!(fields["client_id"], "registered-client");
        assert!(!flow.url().contains("private-client-secret"));
        assert_eq!(
            fields.contains_key("code_challenge"),
            provider != Provider::Dingtalk
        );
        let mut callback = reqwest::Url::parse("http://127.0.0.1:43321/callback").unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", &fields["state"])
            .append_pair("code", "only-this-attempt");
        assert_eq!(
            flow.callback(&Secret::new(callback.as_str().into()))
                .unwrap()
                .expose(),
            "only-this-attempt"
        );
        let other = Flow::begin(
            declaration(provider),
            configuration(provider),
            "http://127.0.0.1:43321/callback",
        )
        .unwrap();
        assert!(
            other
                .callback(&Secret::new(callback.as_str().into()))
                .is_err()
        );
        callback.query_pairs_mut().append_pair("code", "duplicate");
        assert!(
            flow.callback(&Secret::new(callback.as_str().into()))
                .is_err()
        );
        for url in [
            "http://127.0.0.1:43322/callback?code=x&state=y",
            "https://example.com/callback?code=x&state=y",
            "http://127.0.0.1:43321/callback?code=x&state=y#fragment",
        ] {
            assert!(flow.callback(&Secret::new(url.into())).is_err());
        }
    }
}

#[test]
fn rejects_undeclared_provider_authority() {
    for provider in [Provider::Google, Provider::Microsoft, Provider::Dingtalk] {
        let mut declaration = declaration(provider);
        declaration.scopes.push("arbitrary.write".into());
        assert!(
            Flow::begin(
                declaration,
                configuration(provider),
                "http://127.0.0.1:43321/callback"
            )
            .is_err()
        );
        for redirect in [
            "https://example.com/callback",
            "http://127.0.0.1/callback",
            "http://user@localhost:80/callback",
            "http://localhost:80/callback?query=x",
        ] {
            assert!(
                Flow::begin(
                    self::declaration(provider),
                    configuration(provider),
                    redirect
                )
                .is_err()
            );
        }
    }
    let mut config = configuration(Provider::Microsoft);
    config.tenant = Some("common/../../evil".into());
    assert!(
        Flow::begin(
            declaration(Provider::Microsoft),
            config,
            "http://127.0.0.1:43321/callback"
        )
        .is_err()
    );
}
