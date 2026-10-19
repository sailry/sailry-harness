use super::*;
use serde_json::json;

const REDIRECT: &str = "http://127.0.0.1:54321/auth/callback";

fn pending(client_id: Option<&str>) -> Pending {
    let previous = client_id.map(|client_id| Grant::ChatGpt {
        client_id: client_id.into(),
        subject: "subject".into(),
        email: Some("user@example.com".into()),
        id_token: Secret::new("retained-token".into()),
        scopes: vec![],
        access: Secret::new("access".into()),
        refresh: Secret::new("refresh".into()),
        expires_at_ms: 1,
        earliest_refresh_at: None,
        account_id: None,
        renewal_pending: false,
    });
    Pending::new(
        REDIRECT.into(),
        "urn:uuid:fixture-host".into(),
        previous.as_ref(),
        None,
    )
    .unwrap()
}

fn callback(pending: &Pending, values: &[(&str, &str)]) -> Secret {
    let mut url = Url::parse(REDIRECT).unwrap();
    url.query_pairs_mut()
        .append_pair("state", pending.state.expose())
        .extend_pairs(values.iter().copied());
    Secret::new(url.into())
}

#[test]
fn initial_registration_uses_fresh_pkce_and_nonce() {
    let first = pending(None);
    let second = pending(None);
    assert_ne!(first.state, second.state);
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.verifier, second.verifier);
    assert!(first.verifier.expose().len() >= 43);
    let url = Url::parse(&first.url("https://auth.openai.com").unwrap()).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.path(), "/api/accounts/authorize");
    assert_eq!(query["client_id"], REGISTRATION_CLIENT);
    assert_eq!(query["agent_name_hint"], "Sailry");
    assert_eq!(query["ext_agent_host_id"], "urn:uuid:fixture-host");
    assert_eq!(query["resource"], RESOURCE);
    assert_eq!(query["scope"], SCOPES);
    assert_eq!(query["redirect_uri"], REDIRECT);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(first.verifier.expose()))
    );
    assert!(!query.contains_key("id_token_hint"));
    let code = first
        .callback(&callback(
            &first,
            &[("code", "code"), ("client_id", "oaiapp_fixture")],
        ))
        .unwrap();
    assert_eq!(code.client_id, "oaiapp_fixture");
    assert_eq!(code.value.expose(), "code");
}

#[test]
fn returning_registration_preserves_client_without_token_leak() {
    let pending = pending(Some("oaiapp_fixture"));
    let url = Url::parse(&pending.url("https://auth.openai.com").unwrap()).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "oaiapp_fixture");
    assert!(!query.contains_key("agent_name_hint"));
    assert!(!query.contains_key("id_token_hint"));
    assert!(!query.contains_key("login_hint"));
    assert!(!url.as_str().contains("retained-token"));
    let code = pending
        .callback(&callback(&pending, &[("code", "code")]))
        .unwrap();
    assert_eq!(code.client_id, "oaiapp_fixture");
    assert!(
        pending
            .callback(&callback(
                &pending,
                &[("code", "code"), ("client_id", "oaiapp_other")]
            ))
            .is_err()
    );
}

#[test]
fn retries_retained_registration_without_creating_another() {
    let pending = Pending::new(
        REDIRECT.into(),
        "host".into(),
        None,
        Some("oaiapp_retained".into()),
    )
    .unwrap();
    let url = Url::parse(&pending.url("https://auth.openai.com").unwrap()).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "oaiapp_retained");
    assert!(!query.contains_key("agent_name_hint"));
    assert_eq!(
        pending
            .callback(&callback(&pending, &[("code", "fresh-code")]))
            .unwrap()
            .client_id,
        "oaiapp_retained"
    );
    assert!(
        Pending::new(
            REDIRECT.into(),
            "host".into(),
            None,
            Some(REGISTRATION_CLIENT.into())
        )
        .is_err()
    );
}

#[test]
fn rejects_mismatched_and_ambiguous_callbacks() {
    let pending = pending(None);
    for values in [
        vec![("code", "code")],
        vec![("code", "code"), ("client_id", REGISTRATION_CLIENT)],
        vec![("code", ""), ("client_id", "oaiapp_fixture")],
        vec![
            ("code", "code"),
            ("client_id", "oaiapp_fixture"),
            ("code", "other"),
        ],
        vec![
            ("code", "code"),
            ("client_id", "oaiapp_fixture"),
            ("state", "other"),
        ],
    ] {
        assert!(pending.callback(&callback(&pending, &values)).is_err());
    }
    let value = callback(
        &pending,
        &[("code", "code"), ("client_id", "oaiapp_fixture")],
    );
    for altered in [
        value.expose().replace("127.0.0.1", "localhost"),
        value.expose().replace("54321", "54322"),
        value.expose().replace("/auth/callback", "/callback"),
        format!("{}#fragment", value.expose()),
    ] {
        assert!(pending.callback(&Secret::new(altered)).is_err());
    }
    let denied = pending
        .callback(&callback(&pending, &[("error", "access_denied")]))
        .err()
        .unwrap();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let mut wrong = Url::parse(callback(&pending, &[("error", "access_denied")]).expose()).unwrap();
    wrong.set_query(Some("state=other&error=access_denied"));
    assert_eq!(
        pending
            .callback(&Secret::new(wrong.into()))
            .err()
            .unwrap()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn redirect_accepts_only_exact_loopback_path() {
    for redirect in [
        "http://localhost:54321/auth/callback",
        "https://127.0.0.1:54321/auth/callback",
        "http://127.0.0.1:54321/callback",
        "http://127.0.0.1/auth/callback",
        "http://127.0.0.1:0/auth/callback",
        "http://user@127.0.0.1:54321/auth/callback",
        "http://127.0.0.1:54321/auth/callback?query=1",
        "http://127.0.0.1:54321/auth/callback#fragment",
    ] {
        assert!(Pending::new(redirect.into(), "host".into(), None, None).is_err());
    }
}

#[test]
fn legacy_records_are_reported_without_conversion() {
    let old = Secret::new(
        json!({"kind":"chat_gpt", "access":"access", "refresh":"refresh",
        "expires_at_ms":1, "account_id":"account", "renewal_pending":false})
        .to_string(),
    );
    assert!(Grant::decode(Authentication::ChatGpt, &old).is_err());
    assert!(old.expose().contains("account_id"));
}
