//! The stable credential codec outlives package-owned authorization workflows.
use super::*;
use serde_json::{Value, json};

fn document(authentication: Authentication) -> Value {
    let mut value = json!({"kind":serde_json::to_value(authentication).unwrap(),"access":"fixture-access","refresh":"fixture-refresh","expires_at_ms":1800000000000u64,"renewal_pending":false});
    if authentication != Authentication::ClaudeOAuth {
        value["subject"] = json!("subject");
        value["email"] = json!("fixture@example.test");
        value["id_token"] = json!("fixture-identity");
        if authentication == Authentication::ChatGpt {
            value["client_id"] = json!("oaiapp_fixture");
            value["scopes"] = json!([
                "openid",
                "profile",
                "email",
                "offline_access",
                "resource.invoke",
                "chatgpt.tokens.use.direct"
            ]);
            value["earliest_refresh_at"] = Value::Null;
        } else {
            value["account_id"] = json!("account");
        }
    }
    value
}
fn read(authentication: Authentication, document: Value) -> Result<Grant, Fault> {
    Grant::decode(authentication, &Secret::new(document.to_string()))
}

#[test]
fn preserves_native_credentials_and_renewal_fences() {
    for authentication in [
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::ClaudeOAuth,
    ] {
        let value = document(authentication);
        let mut grant = read(authentication, value.clone()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(grant.encode().unwrap().expose()).unwrap(),
            value
        );
        assert!(!grant.renewal_pending());
        grant.begin_renewal();
        let recovered = Grant::decode(authentication, &grant.encode().unwrap()).unwrap();
        assert!(recovered.renewal_pending());
        assert!(recovered.same_account(&grant));
        assert!(super::super::transferable(authentication).is_err());
    }
}
#[test]
fn keeps_captured_backend_headers() {
    let grant = read(
        Authentication::ChatGptOAuth,
        document(Authentication::ChatGptOAuth),
    )
    .unwrap();
    let options = Options::ChatGptOAuth {
        catalog_version: Some("0.161.0".into()),
        user_agent: "CapturedAgent/1".into(),
    };
    let headers = grant.headers(&options).unwrap();
    assert_eq!(headers["authorization"], "Bearer fixture-access");
    assert!(headers["authorization"].is_sensitive());
    assert_eq!(headers["chatgpt-account-id"], "account");
    assert_eq!(headers["originator"], "codex_cli_rs");
    assert_eq!(headers["version"], "0.161.0");
    assert_eq!(headers["user-agent"], "CapturedAgent/1");
    assert_eq!(grant.endpoint(), CODEX_ENDPOINT);
    assert!(!grant.plan_enabled());
    assert!(
        grant
            .headers(&Options::defaults(Authentication::ChatGpt).unwrap())
            .is_err()
    );
    assert!(Grant::decode(Authentication::ChatGpt, &grant.encode().unwrap()).is_err());
}
#[test]
fn retains_unentitled_siwc_identity() {
    for scopes in [
        json!(["openid", "resource.invoke"]),
        json!(["chatgpt.tokens.use.direct"]),
        json!([]),
    ] {
        let mut value = document(Authentication::ChatGpt);
        value["scopes"] = scopes;
        let grant = read(Authentication::ChatGpt, value).unwrap();
        assert!(!grant.plan_enabled());
        assert_eq!(
            grant
                .headers(&Options::defaults(Authentication::ChatGpt).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(Grant::decode(Authentication::ChatGpt, &grant.encode().unwrap()).is_ok());
    }
}
#[test]
fn separates_account_and_registration_identity() {
    for authentication in [Authentication::ChatGpt, Authentication::ChatGptOAuth] {
        let value = document(authentication);
        let first = read(authentication, value.clone()).unwrap();
        for field in if authentication == Authentication::ChatGpt {
            vec!["subject", "client_id"]
        } else {
            vec!["subject", "account_id"]
        } {
            let mut changed = value.clone();
            changed[field] = json!("other-identity");
            let second = read(authentication, changed).unwrap();
            assert!(!first.same_account(&second));
        }
    }
}
#[test]
fn rejects_invalid_native_records() {
    for authentication in [
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::ClaudeOAuth,
    ] {
        for (field, invalid) in [
            ("access", json!("")),
            ("refresh", json!("")),
            ("expires_at_ms", json!(0)),
            ("expires_at_ms", json!(u64::MAX)),
        ] {
            let mut value = document(authentication);
            value[field] = invalid;
            assert!(read(authentication, value).is_err());
        }
    }
    let legacy = json!({"kind":"chat_gpt","access":"access","refresh":"refresh","expires_at_ms":1,"account_id":"account","renewal_pending":false});
    assert!(read(Authentication::ChatGpt, legacy).is_err());
}
#[test]
fn retains_optional_resource_account_identity() {
    let mut value = document(Authentication::ChatGpt);
    value["account_id"] = json!("resource-account");
    let grant = read(Authentication::ChatGpt, value.clone()).unwrap();
    assert_eq!(
        grant
            .resource_headers("https://chatgpt.com/backend-api/wham/usage")
            .unwrap()["chatgpt-account-id"],
        "resource-account"
    );
    value.as_object_mut().unwrap().remove("account_id");
    let grant = read(Authentication::ChatGpt, value).unwrap();
    assert!(
        !grant
            .resource_headers("https://chatgpt.com/backend-api/wham/usage")
            .unwrap()
            .contains_key("chatgpt-account-id")
    );
    assert!(
        serde_json::from_str::<Value>(grant.encode().unwrap().expose())
            .unwrap()
            .get("account_id")
            .is_none()
    );
}
