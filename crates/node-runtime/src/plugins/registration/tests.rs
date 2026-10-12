use super::*;
use std::{collections::BTreeMap, io::Read};
#[path = "../../../tests/plugins/registration/server.rs"]
mod server;

fn declaration() -> Declaration {
    Declaration {
        provider: sailry_protocol::plugin::registration::Provider::Feishu,
        client_id: "app_id".into(),
        client_secret: "app_secret".into(),
        tenant_scopes: vec![
            "docx:document:readonly".into(),
            "docs:document.content:read".into(),
            "docs:document.comment:read".into(),
        ],
    }
}

#[test]
fn requires_protected_application_binding() {
    let schema: Schema = serde_json::from_value(json!({
        "$schema":sailry_protocol::plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{
            "app_id":{"type":"string"},
            "app_secret":{"type":"string","x-sailry-secret":{"server":"feishu","header":"X-Lark-MCP-TAT","application":{"provider":"feishu","client_id":"app_id"}}}
        },"required":["app_id","app_secret"]
    })).unwrap();
    assert!(validate(&declaration(), Some(&schema)).is_ok());
    assert!(validate(&declaration(), None).is_err());
    let mut public_secret = schema.clone();
    public_secret
        .properties
        .get_mut("app_secret")
        .unwrap()
        .secret = None;
    assert!(validate(&declaration(), Some(&public_secret)).is_err());
    let mut wrong_id = schema.clone();
    wrong_id.properties.get_mut("app_id").unwrap().kind =
        sailry_protocol::plugin::settings::Kind::Integer;
    assert!(validate(&declaration(), Some(&wrong_id)).is_err());
    let mut mismatched = schema;
    mismatched
        .properties
        .get_mut("app_secret")
        .unwrap()
        .secret
        .as_mut()
        .unwrap()
        .application
        .as_mut()
        .unwrap()
        .client_id = "other_app".into();
    assert!(validate(&declaration(), Some(&mismatched)).is_err());
}

#[test]
fn constrains_consent_and_verification() {
    let declared = declaration();
    let url = verification(&json!("https://accounts.feishu.cn/registration?user_code=fixture&addons=bad&clientID=old&createOnly=false"), &declared).unwrap();
    let url = Url::parse(&url).unwrap();
    let fields: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(fields["createOnly"], "true");
    assert!(!fields.contains_key("clientID"));
    assert_eq!(fields["user_code"], "fixture");
    let compressed = URL_SAFE_NO_PAD.decode(&fields["addons"]).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"preset":false,"scopes":{"tenant":declared.tenant_scopes}})
    );
    assert!(!text.contains("events"));
    for value in [
        "http://accounts.feishu.cn/",
        "https://evil.test/",
        "https://accounts.feishu.cn.evil.test/",
        "https://name@accounts.feishu.cn/",
        "https://accounts.feishu.cn:444/",
        "https://accounts.feishu.cn/#secret",
    ] {
        assert!(verification(&json!(value), &declaration()).is_err());
    }
    let mut invalid = declaration();
    invalid.tenant_scopes.push(invalid.tenant_scopes[0].clone());
    assert!(!invalid.valid());
    invalid.tenant_scopes = vec!["scope\nsecret".into()];
    assert!(!invalid.valid());
}

#[tokio::test]
async fn honors_provider_backoff() {
    let issuer = server::Server::start(vec![
        json!({"error":"slow_down"}),
        server::Server::success(),
    ])
    .await;
    let service = Service::default();
    service.fixture(&issuer.endpoint);
    let credentials = service
        .authorize(
            declaration(),
            |url, expiry| async move {
                assert!(url.starts_with("https://accounts.feishu.cn/"));
                assert!(expiry > now().unwrap());
                assert!(!url.contains("private-device-marker"));
                Ok(())
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(credentials.client_id, "cli_registered");
    assert_eq!(
        credentials.client_secret.expose(),
        "private-app-secret-marker"
    );
    let records = issuer.requests.lock().unwrap();
    assert_eq!(records.len(), 4);
    assert!(records[3].0.duration_since(records[2].0) >= Duration::from_secs(6));
}

#[tokio::test]
async fn rejects_private_faults_and_unsupported_tenants() {
    for (response, code) in [
        (
            json!({"error":"access_denied", "error_description":"private-app-secret-marker"}),
            ErrorCode::PermissionDenied,
        ),
        (json!({"error":"expired_token"}), ErrorCode::Expired),
        (
            json!({"error":"unknown", "error_description":"private-app-secret-marker"}),
            ErrorCode::OutcomeUnknown,
        ),
        (
            json!({"user_info":{"tenant_brand":"lark"},"client_id":"cli_lark","client_secret":"private-app-secret-marker"}),
            ErrorCode::NotConfigured,
        ),
        (
            json!({"client_id":"not-an-app","client_secret":"private-app-secret-marker"}),
            ErrorCode::OutcomeUnknown,
        ),
    ] {
        let issuer = server::Server::start(vec![response]).await;
        let service = Service::default();
        service.fixture(&issuer.endpoint);
        let error = service
            .authorize(
                declaration(),
                |_, _| async { Ok(()) },
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("private-app-secret-marker"));
        assert_eq!(issuer.requests.lock().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn cancels_without_starting_another_attempt() {
    let issuer = server::Server::start(vec![]).await;
    let service = Service::default();
    service.fixture(&issuer.endpoint);
    let stop = CancellationToken::new();
    let error = service
        .authorize(
            declaration(),
            |_, _| {
                stop.cancel();
                async { Ok(()) }
            },
            &stop,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(issuer.requests.lock().unwrap().len() <= 3);
}
