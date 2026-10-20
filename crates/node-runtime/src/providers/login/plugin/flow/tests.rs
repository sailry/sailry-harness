use super::*;
use serde_json::json;

#[test]
fn binds_pkce_to_exact_callback_and_state() {
    let redirect = "http://127.0.0.1:12345/auth/callback";
    validate_redirect(redirect, model_auth::Redirect::Loopback).unwrap();
    let fields = BTreeMap::from([("client_id".into(), Secret::new("example-client".into()))]);
    let pending = Pending::new(
        "https://auth.example.test/authorize".into(),
        redirect.into(),
        &fields,
        &["profile".into()],
    )
    .unwrap();
    let url = Url::parse(&pending.url).unwrap();
    let query = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.expose().as_bytes()))
    );
    assert!(!pending.url.contains(pending.verifier.expose()));
    assert_eq!(query["scope"], "profile");
    let mut callback = Url::parse(redirect).unwrap();
    callback
        .query_pairs_mut()
        .extend_pairs([("code", "private-code"), ("state", pending.state.expose())]);
    assert_eq!(
        pending
            .code(&Secret::new(callback.to_string()))
            .unwrap()
            .expose(),
        "private-code"
    );
    let mut wrong = callback.clone();
    wrong.set_port(Some(12346)).unwrap();
    assert!(pending.code(&Secret::new(wrong.to_string())).is_err());
    assert!(
        pending
            .code(&Secret::new(format!("{callback}&state=duplicate")))
            .is_err()
    );
    assert!(
        pending
            .code(&Secret::new(format!(
                "{redirect}?code=private-code&state=wrong"
            )))
            .is_err()
    );
    assert!(
        Pending::new(
            "https://auth.example.test/authorize?state=unsafe".into(),
            redirect.into(),
            &fields,
            &[]
        )
        .is_err()
    );
}

#[test]
fn validates_device_progress_without_private_code() {
    let document = json!({"verification_uri":"https://auth.example.test/device","user_code":"ABCD-EFGH","device_code":"private-device-code","expires_in":600,"interval":3});
    let device = Device::parse(&document, 1000).unwrap();
    assert_eq!(device.expires_at_ms, 601000);
    assert_eq!(device.interval, Duration::from_secs(3));
    let progress = State::Pending {
        verification_url: device.url,
        user_code: device.user_code,
        expires_at_ms: device.expires_at_ms,
    };
    assert!(
        !serde_json::to_string(&progress)
            .unwrap()
            .contains(device.code.expose())
    );
    for field in ["user_code", "device_code", "expires_in"] {
        let mut invalid = document.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(Device::parse(&invalid, 1000).is_err());
    }
    let mut invalid = document;
    invalid["verification_uri"] = json!("http://public.example.test/device");
    assert!(Device::parse(&invalid, 0).is_err());
}

#[test]
fn preserves_declared_body_and_private_substitution() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let endpoint = model_auth::Endpoint {
        cache_ttl_ms: None,
        url: "https://auth.example.test/token".into(),
        method: model_auth::Method::Post,
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
        body: None,
    };
    let fields = BTreeMap::from([
        ("client_id".into(), Secret::new("example".into())),
        ("refresh".into(), Secret::new("private-refresh".into())),
    ]);
    let endpoint = form(
        endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", "{refresh}"),
        ],
        &fields,
    );
    let model_auth::Body::Form { fields: body } = endpoint.body.as_ref().unwrap() else {
        panic!("expected a form");
    };
    assert_eq!(body["refresh_token"], "{refresh}");
    let prepared = prepare(&reqwest::Client::new(), &endpoint, &fields)
        .unwrap()
        .build()
        .unwrap();
    let body = std::str::from_utf8(prepared.body().unwrap().as_bytes().unwrap()).unwrap();
    let body = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(body["refresh_token"], "private-refresh");
    assert_eq!(body["client_id"], "example");
    assert_eq!(
        form(endpoint.clone(), &[("extra", "unused")], &fields),
        endpoint
    );
}

#[tokio::test]
async fn rejects_invalid_private_grants() {
    for missing in [true, false] {
        let mut grant = json!({
            "kind":"chat_gpt_oauth","access":"PRIVATE_ACCESS","refresh":"PRIVATE_REFRESH",
            "expires_at_ms":0,"subject":"PRIVATE_SUBJECT","account_id":"PRIVATE_ACCOUNT",
            "email":"private@example.test","id_token":"PRIVATE_ID_TOKEN","renewal_pending":false,
        });
        if missing {
            grant.as_object_mut().unwrap().remove("id_token");
        }
        let entry = "dev.sailry.platform/auth/fixture.js";
        let declaration: model_auth::Declaration = serde_json::from_value(json!({
            "id":"fixture","authentication":"chat_gpt_oauth","display":{"label":"Fixture"},
            "flow":{"kind":"script","script":{"entry":entry,"resources":[]},"handler":"authorize",
                "requests":{"token":{"url":"https://auth.example.test/token"}},"callback":null,"issuer":null},
        })).unwrap();
        assert!(declaration.valid());
        let bundle = crate::plugins::script::Bundle {
            entry: entry.into(),
            files: BTreeMap::from([(
                entry.into(),
                format!(
                    "export function authorize() {{ return {}; }}",
                    json!({"kind":"finish","grant":grant})
                ),
            )]),
        };
        let result = authorize(
            Input {
                context: None,
                service: crate::providers::login::Service::default(),
                declaration,
                fields: BTreeMap::new(),
                redirect: None,
                callback: None,
                exchange: Some(bundle),
            },
            |_| async { panic!("private finish does not prompt") },
            CancellationToken::new(),
        )
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("invalid private grant was accepted"),
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            if missing {
                "model authorization data is invalid"
            } else {
                "authorization expiry is invalid"
            }
        );
        let public = serde_json::to_string(&error).unwrap();
        assert!(!public.contains("PRIVATE"));
        assert!(!public.contains("private@example.test"));
    }
}

#[tokio::test]
async fn rejects_identity_changes_during_renewal() {
    let original = json!({"kind":"chat_gpt_oauth","access":"original-access","refresh":"original-refresh","expires_at_ms":1800000000000u64,"subject":"original-subject","account_id":"original-account","email":"original@example.test","id_token":"original-id-token","renewal_pending":true});
    let previous = super::super::super::Grant::decode(
        sailry_protocol::Authentication::ChatGptOAuth,
        &Secret::new(original.to_string()),
    )
    .unwrap();
    for field in ["subject", "account_id"] {
        let mut changed = original.clone();
        changed[field] = json!("changed-identity");
        let entry = "dev.sailry.platform/auth/fixture.js";
        let declaration: model_auth::Declaration = serde_json::from_value(json!({"id":"fixture","authentication":"chat_gpt_oauth","display":{"label":"Fixture"},"flow":{"kind":"script","script":{"entry":entry,"resources":[]},"handler":"authorize","refresh":true,"requests":{"token":{"url":"https://auth.example.test/token"}}}})).unwrap();
        let bundle = crate::plugins::script::Bundle {
            entry: entry.into(),
            files: BTreeMap::from([(
                entry.into(),
                format!(
                    "export function authorize() {{ return {}; }}",
                    json!({"kind":"finish","grant":changed})
                ),
            )]),
        };
        let error = renew(
            Input {
                context: None,
                service: crate::providers::login::Service::default(),
                declaration,
                fields: BTreeMap::new(),
                redirect: None,
                callback: None,
                exchange: Some(bundle),
            },
            &previous,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            serde_json::from_str::<Value>(previous.encode().unwrap().expose()).unwrap(),
            original
        );
    }
}
