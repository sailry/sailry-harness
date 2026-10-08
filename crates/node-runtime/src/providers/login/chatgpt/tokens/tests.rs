use super::*;
use serde_json::{Value, json};

fn response() -> Value {
    json!({"access_token":"access", "refresh_token":"refresh", "id_token":"identity",
        "token_type":"Bearer", "expires_in":3600, "scope":SCOPES})
}

fn grant(value: Value, previous: Option<&Grant>) -> Result<Grant, Fault> {
    serde_json::from_value::<Tokens>(value).unwrap().grant(
        "oaiapp_fixture".into(),
        identity::Identity {
            sub: "subject".into(),
            email: Some("user@example.com".into()),
            account_id: None,
        },
        previous,
    )
}

#[test]
fn expiry_comes_from_response_not_access_claims() {
    let before = now_ms().unwrap();
    let value = grant(response(), None).unwrap();
    assert!(value.expires_at_ms() >= before + 3_600_000);
    assert!(value.expires_at_ms() <= now_ms().unwrap() + 3_600_000);
    let encoded = value.encode().unwrap();
    assert!(Grant::decode(Authentication::ChatGpt, &encoded).is_ok());
}

#[test]
fn retains_identity_without_plan_permission() {
    for scope in [
        "openid email offline_access resource.invoke",
        "chatgpt.tokens.use.direct",
        "",
    ] {
        let mut value = response();
        value["scope"] = json!(scope);
        let retained = grant(value, None).unwrap();
        assert!(!retained.plan_enabled());
        let fault = retained
            .headers(&Options::defaults(Authentication::ChatGpt).unwrap())
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::NotConfigured);
        assert!(Grant::decode(Authentication::ChatGpt, &retained.encode().unwrap()).is_ok());
    }
    let mut missing = response();
    missing.as_object_mut().unwrap().remove("scope");
    assert!(grant(missing, None).is_err());
}

#[test]
fn preserves_refresh_hint_without_interpreting_units() {
    let mut response = response();
    response["earliest_refresh_at"] = json!(1790036100);
    let Grant::ChatGpt {
        earliest_refresh_at,
        ..
    } = grant(response, None).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(earliest_refresh_at, Some(1790036100));
}

#[test]
fn refresh_preserves_identity_and_rotates_both_tokens() {
    let previous = grant(response(), None).unwrap();
    let mut value = response();
    value["access_token"] = json!("next-access");
    value["refresh_token"] = json!("next-refresh");
    value.as_object_mut().unwrap().remove("id_token");
    value.as_object_mut().unwrap().remove("scope");
    let refreshed = grant(value, Some(&previous)).unwrap();
    let Grant::ChatGpt {
        client_id,
        subject,
        id_token,
        refresh,
        scopes,
        ..
    } = &refreshed
    else {
        unreachable!()
    };
    assert_eq!(client_id, "oaiapp_fixture");
    assert_eq!(subject, "subject");
    assert_eq!(id_token.expose(), "identity");
    assert_eq!(refresh.expose(), "next-refresh");
    assert_eq!(refreshed.access().expose(), "next-access");
    assert_eq!(scopes.len(), 6);
    assert!(refreshed.same_account(&previous));
    let headers = refreshed
        .headers(&Options::defaults(Authentication::ChatGpt).unwrap())
        .unwrap();
    assert_eq!(headers["authorization"], "Bearer next-access");
    assert!(headers["authorization"].is_sensitive());
    assert!(!headers.contains_key("chatgpt-account-id"));
    assert!(!headers.contains_key("originator"));
}

#[test]
fn separates_registrations_with_the_same_email() {
    let first = grant(response(), None).unwrap();
    let mut second = grant(response(), None).unwrap();
    let Grant::ChatGpt { client_id, .. } = &mut second else {
        unreachable!()
    };
    *client_id = "oaiapp_other_workspace".into();
    assert!(!first.same_account(&second));
    let Grant::ChatGpt {
        client_id, subject, ..
    } = &mut second
    else {
        unreachable!()
    };
    *client_id = "oaiapp_fixture".into();
    *subject = "other-subject".into();
    assert!(!first.same_account(&second));
}

#[test]
fn refuses_invalid_expiry_type_and_missing_rotation() {
    for (field, value) in [
        ("expires_in", json!(0)),
        ("expires_in", json!(u64::MAX)),
        ("token_type", json!("Basic")),
    ] {
        let mut response = response();
        response[field] = value;
        assert!(grant(response, None).is_err());
    }
    let mut value = response();
    value.as_object_mut().unwrap().remove("refresh_token");
    assert!(serde_json::from_value::<Tokens>(value).is_err());
}
