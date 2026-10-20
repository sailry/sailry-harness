use super::*;

fn response() -> Value {
    json!({"access_token":"access", "refresh_token":"refresh", "id_token":"identity",
        "token_type":"Bearer", "expires_in":3600})
}

fn identity() -> identity::Identity {
    identity::Identity {
        sub: "subject".into(),
        email: Some("user@example.com".into()),
        account_id: Some("workspace".into()),
    }
}

fn grant(value: Value, previous: Option<&Grant>) -> Result<Grant, Fault> {
    serde_json::from_value::<Tokens>(value)
        .unwrap()
        .grant(identity(), previous)
}

#[test]
fn bounds_poll_intervals() {
    for (value, expected) in [
        (None, 5),
        (Some(json!("2")), 2),
        (Some(json!(3)), 3),
        (Some(json!(0)), 1),
        (Some(json!(1000)), 900),
        (Some(json!("invalid")), 5),
        (Some(json!(-1)), 5),
    ] {
        assert_eq!(interval(value.as_ref()), expected);
    }
}

#[test]
fn preserves_distinct_grant_and_endpoint() {
    let before = now_ms().unwrap();
    let value = grant(response(), None).unwrap();
    assert!(value.expires_at_ms() >= before + 3_600_000);
    assert!(value.expires_at_ms() <= now_ms().unwrap() + 3_600_000);
    assert_eq!(value.authentication(), Authentication::ChatGptOAuth);
    assert_eq!(value.endpoint(), CODEX_ENDPOINT);
    assert!(!value.plan_enabled());
    let encoded = value.encode().unwrap();
    assert!(Grant::decode(Authentication::ChatGptOAuth, &encoded).is_ok());
    assert!(Grant::decode(Authentication::ChatGpt, &encoded).is_err());
    let serialized: Value = serde_json::from_str(encoded.expose()).unwrap();
    assert_eq!(serialized["kind"], "chat_gpt_oauth");
    for field in ["client_id", "scopes", "earliest_refresh_at"] {
        assert!(serialized.get(field).is_none());
    }
}

#[test]
fn sends_captured_backend_identity() {
    let value = grant(response(), None).unwrap();
    let options = Options::ChatGptOAuth {
        catalog_version: Some("0.161.0".into()),
        user_agent: "CapturedAgent/1".into(),
    };
    let headers = value.headers(&options).unwrap();
    assert_eq!(headers["authorization"], "Bearer access");
    assert!(headers["authorization"].is_sensitive());
    assert_eq!(headers["chatgpt-account-id"], "workspace");
    assert_eq!(headers["originator"], "codex_cli_rs");
    assert_eq!(headers["version"], "0.161.0");
    assert_eq!(headers["user-agent"], "CapturedAgent/1");
    assert!(
        value
            .headers(&Options::defaults(Authentication::ChatGpt).unwrap())
            .is_err()
    );
    assert!(transferable(Authentication::ChatGptOAuth).is_err());
}

#[test]
fn marks_refresh_before_rotation() {
    let mut previous = grant(response(), None).unwrap();
    assert!(!previous.renewal_pending());
    previous.begin_renewal();
    assert!(previous.renewal_pending());
    let retained =
        Grant::decode(Authentication::ChatGptOAuth, &previous.encode().unwrap()).unwrap();
    assert!(retained.renewal_pending());
    let mut value = response();
    value["access_token"] = json!("next-access");
    value["refresh_token"] = json!("next-refresh");
    value["id_token"] = json!("next-identity");
    let refreshed = grant(value, Some(&previous)).unwrap();
    assert!(!refreshed.renewal_pending());
    assert!(refreshed.same_account(&previous));
    let Grant::ChatGptOAuth {
        access,
        refresh,
        id_token,
        ..
    } = refreshed
    else {
        unreachable!()
    };
    assert_eq!(access.expose(), "next-access");
    assert_eq!(refresh.expose(), "next-refresh");
    assert_eq!(id_token.expose(), "next-identity");
}

#[test]
fn retains_unreplaced_refresh_fields() {
    let previous = grant(response(), None).unwrap();
    let mut value = response();
    value["access_token"] = json!("next-access");
    for field in ["refresh_token", "id_token", "token_type"] {
        value.as_object_mut().unwrap().remove(field);
    }
    let refreshed = grant(value.clone(), Some(&previous)).unwrap();
    assert!(refreshed.same_account(&previous));
    let Grant::ChatGptOAuth {
        access,
        refresh,
        id_token,
        ..
    } = refreshed
    else {
        unreachable!()
    };
    assert_eq!(access.expose(), "next-access");
    assert_eq!(refresh.expose(), "refresh");
    assert_eq!(id_token.expose(), "identity");
    assert!(grant(value, None).is_err());
}

#[test]
fn rejects_invalid_tokens_and_identity() {
    for (field, value) in [
        ("access_token", json!("")),
        ("refresh_token", json!("")),
        ("id_token", json!("")),
        ("expires_in", json!(0)),
        ("expires_in", json!(u64::MAX)),
        ("token_type", json!("Basic")),
    ] {
        let mut response = response();
        response[field] = value;
        assert!(grant(response, None).is_err(), "{field}");
    }
    for account_id in [
        None,
        Some("".into()),
        Some("workspace\r\nheader: value".into()),
    ] {
        let mut identity = identity();
        identity.account_id = account_id;
        assert!(
            serde_json::from_value::<Tokens>(response())
                .unwrap()
                .grant(identity, None)
                .is_err()
        );
    }
}

#[test]
fn rejects_subject_or_workspace_change() {
    let previous = grant(response(), None).unwrap();
    for change_subject in [false, true] {
        let mut identity = identity();
        if change_subject {
            identity.sub = "other-subject".into();
        } else {
            identity.account_id = Some("other-workspace".into());
        }
        assert!(
            serde_json::from_value::<Tokens>(response())
                .unwrap()
                .grant(identity, Some(&previous))
                .is_err()
        );
    }
}

#[test]
fn accepts_device_code_spellings() {
    for field in ["user_code", "usercode"] {
        let mut value = json!({"device_auth_id":"fixture-device","interval":"5"});
        value[field] = json!("ABCD-EFGH");
        let device: Device = serde_json::from_value(value).unwrap();
        assert_eq!(device.user_code, "ABCD-EFGH");
        assert_eq!(interval(device.interval.as_ref()), 5);
    }
}

#[test]
fn recognizes_pending_responses() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::TOO_MANY_REQUESTS] {
        for (code, expected) in [
            ("deviceauth_authorization_pending", Poll::Pending),
            ("slow_down", Poll::SlowDown),
        ] {
            for value in [json!({"error":code}), json!({"error":{"code":code}})] {
                assert_eq!(
                    pending(status, &serde_json::to_vec(&value).unwrap()),
                    Some(expected)
                );
                assert_eq!(
                    pending(StatusCode::OK, &serde_json::to_vec(&value).unwrap()),
                    None
                );
            }
        }
    }
    for status in [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND] {
        assert_eq!(pending(status, b""), Some(Poll::Pending));
    }
    for value in [
        json!({"error":"access_denied"}),
        json!({"error":"invalid_grant"}),
        json!({"error":{"code":false}}),
        json!({}),
    ] {
        assert_eq!(
            pending(
                StatusCode::BAD_REQUEST,
                &serde_json::to_vec(&value).unwrap()
            ),
            None
        );
    }
    assert_eq!(
        pending(StatusCode::BAD_REQUEST, b"private upstream body"),
        None
    );
}
