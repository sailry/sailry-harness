use super::*;
use serde_json::json;

fn mapping() -> model_auth::Mapping {
    model_auth::Mapping {
        access: "/data/access".into(),
        refresh: Some("/data/refresh".into()),
        expires_in: Some("/data/expires_in".into()),
        account_label: None,
        extra: BTreeMap::from([("account".into(), "/data/account".into())]),
    }
}

fn issued() -> Issued {
    Issued::parse(&json!({"data":{"access":"private-access","refresh":"private-refresh","expires_in":60,"account":"private-account"}}), &mapping(), 1000, None).unwrap()
}

#[test]
fn binds_attempt_revision_and_package_settings() {
    let capture = Capture {
        attempt: RequestId::new(),
        provider: ProviderId::new(),
        revision: 3,
        package: Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 2,
        },
        flow: "example".into(),
        origin: sailry_protocol::conversation::consent::Origin::Bundled,
    };
    assert!(
        capture
            .check(
                capture.attempt,
                capture.provider,
                3,
                &capture.package,
                &capture.origin,
                "example"
            )
            .is_ok()
    );
    assert_eq!(
        capture
            .check(
                RequestId::new(),
                capture.provider,
                3,
                &capture.package,
                &capture.origin,
                "example"
            )
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        capture
            .check(
                capture.attempt,
                ProviderId::new(),
                3,
                &capture.package,
                &capture.origin,
                "example"
            )
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        capture
            .check(
                capture.attempt,
                capture.provider,
                4,
                &capture.package,
                &capture.origin,
                "example"
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    for package in [
        Reference {
            digest: "b".repeat(64),
            ..capture.package.clone()
        },
        Reference {
            settings_revision: 3,
            ..capture.package.clone()
        },
    ] {
        assert_eq!(
            capture
                .check(
                    capture.attempt,
                    capture.provider,
                    3,
                    &package,
                    &capture.origin,
                    "example"
                )
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    assert_eq!(
        capture
            .check(
                capture.attempt,
                capture.provider,
                3,
                &capture.package,
                &sailry_protocol::conversation::consent::Origin::Imported {
                    digest: capture.package.digest.clone()
                },
                "example"
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn maps_private_tokens_without_disclosing_values() {
    let issued = issued();
    assert_eq!(issued.expires_at_ms, Some(61000));
    let debug = format!("{issued:?}");
    for secret in issued.fields().values() {
        assert!(!debug.contains(secret.expose()));
    }
    let declarations = [
        model_auth::Injection {
            header: "Authorization".into(),
            template: "Bearer {access}".into(),
        },
        model_auth::Injection {
            header: "X-Account".into(),
            template: "{extra.account}".into(),
        },
    ];
    let headers = issued.headers(&declarations).unwrap();
    assert_eq!(headers["authorization"], "Bearer private-access");
    assert_eq!(headers["x-account"], "private-account");
    assert!(headers["authorization"].is_sensitive());
    assert!(!format!("{headers:?}").contains("private-access"));
    assert!(
        issued
            .headers(&[model_auth::Injection {
                header: "host".into(),
                template: "{access}".into()
            }])
            .is_err()
    );
}

#[test]
fn retains_refresh_and_extra_when_omitted() {
    let previous = issued();
    let updated = Issued::parse(
        &json!({"data":{"access":"next-access","expires_in":120}}),
        &mapping(),
        2000,
        Some(&previous),
    )
    .unwrap();
    assert_eq!(updated.access.expose(), "next-access");
    assert_eq!(updated.refresh, previous.refresh);
    assert_eq!(updated.extra, previous.extra);
    assert_eq!(updated.expires_at_ms, Some(122000));
    for value in [
        json!(-1),
        json!(0),
        json!(0.5),
        json!("60"),
        json!(u64::MAX),
    ] {
        assert!(
            Issued::parse(
                &json!({"data":{"access":"secret","expires_in":value}}),
                &mapping(),
                1,
                None
            )
            .is_err()
        );
    }
    assert!(Issued::parse(&json!({"data":{"expires_in":60}}), &mapping(), 1, None).is_err());
}

#[test]
fn retains_accounts_only_for_the_same_contract() {
    let endpoint = model_auth::Endpoint {
        cache_ttl_ms: None,
        url: "https://auth.example.test/token".into(),
        method: model_auth::Method::Post,
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
        body: None,
    };
    let target = Target {
        digest: Some("a".repeat(64)),
        provider: ProviderId::new(),
        package: "example".into(),
        origin: sailry_protocol::conversation::consent::Origin::Bundled,
        flow: "example".into(),
        endpoint: "https://models.example.test".into(),
        authorization: model_auth::Flow::TokenExchange {
            token: endpoint.clone(),
        },
        refresh: Some(endpoint),
        inject: vec![model_auth::Injection {
            header: "Authorization".into(),
            template: "Bearer {access}".into(),
        }],
        fields: BTreeMap::from([("client_id".into(), Secret::new("private-client".into()))]),
        scopes: vec!["model".into()],
    };
    let record = Record {
        target: target.clone(),
        issued: issued(),
    };
    record.validate().unwrap();
    record.matches(&target).unwrap();
    for changed in [
        Target {
            digest: None,
            ..target.clone()
        },
        Target {
            digest: Some("b".repeat(64)),
            ..target.clone()
        },
        Target {
            origin: sailry_protocol::conversation::consent::Origin::Imported {
                digest: "b".repeat(64),
            },
            ..target.clone()
        },
    ] {
        record.matches(&changed).unwrap();
    }
    for changed in [
        Target {
            provider: ProviderId::new(),
            ..target.clone()
        },
        Target {
            package: "other".into(),
            ..target.clone()
        },
        Target {
            endpoint: "https://other.example.test".into(),
            ..target.clone()
        },
        Target {
            flow: "other".into(),
            ..target.clone()
        },
        Target {
            scopes: vec!["other".into()],
            ..target.clone()
        },
        Target {
            fields: BTreeMap::from([("client_id".into(), Secret::new("changed-client".into()))]),
            ..target.clone()
        },
        Target {
            authorization: model_auth::Flow::TokenExchange {
                token: model_auth::Endpoint {
                    cache_ttl_ms: None,
                    url: "https://other.example.test/token".into(),
                    ..target.refresh.clone().unwrap()
                },
            },
            ..target.clone()
        },
        Target {
            inject: vec![model_auth::Injection {
                header: "X-Key".into(),
                template: "{access}".into(),
            }],
            ..target.clone()
        },
    ] {
        assert_eq!(
            record.matches(&changed).unwrap_err().code,
            ErrorCode::NotConfigured
        );
    }
    let debug = format!("{record:?}");
    assert!(!debug.contains("private-client"));
    assert!(
        crate::providers::login::transferable(sailry_protocol::Authentication::Plugin).is_err()
    );
}

#[test]
fn prepares_header_refresh_without_exposing_query_values() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let endpoint = model_auth::Endpoint {
        cache_ttl_ms: None,
        url: "https://example.test/refresh".into(),
        method: model_auth::Method::Post,
        headers: BTreeMap::from([("X-Refresh-Token".into(), "{refresh}".into())]),
        query: BTreeMap::from([("client".into(), "{client_id}".into())]),
        body: Some(model_auth::Body::Json {
            value: json!({"account":"{extra.account}","nested":[{"value":true}]}),
        }),
    };
    assert!(endpoint.valid());
    let mut fields = issued().fields();
    fields.insert("client_id".into(), Secret::new("client /+&".into()));
    let request = prepare(&reqwest::Client::new(), &endpoint, &fields)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        request.url().query_pairs().collect::<Vec<_>>(),
        [("client".into(), "client /+&".into())]
    );
    assert_eq!(request.headers()["x-refresh-token"], "private-refresh");
    assert!(request.headers()["x-refresh-token"].is_sensitive());
    assert_eq!(
        serde_json::from_slice::<Value>(request.body().unwrap().as_bytes().unwrap()).unwrap(),
        json!({"account":"private-account","nested":[{"value":true}]})
    );
    assert!(prepare(&reqwest::Client::new(), &endpoint, &BTreeMap::new()).is_err());
}

#[test]
fn refuses_invalid_headers_without_value_echo() {
    let issued = Issued {
        access: Secret::new("private\r\nvalue".into()),
        ..issued()
    };
    let result = issued.headers(&[model_auth::Injection {
        header: "Authorization".into(),
        template: "Bearer {access}".into(),
    }]);
    let error = result.unwrap_err();
    assert!(!serde_json::to_string(&error).unwrap().contains("private"));
    assert!(
        Issued::parse(
            &json!({"data":{"access":123,"expires_in":60}}),
            &mapping(),
            0,
            None
        )
        .is_err()
    );
}

#[tokio::test]
async fn distinguishes_an_undelivered_refresh() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let request = model_auth::Endpoint {
        cache_ttl_ms: None,
        url: "https://auth.example.test/refresh".into(),
        method: model_auth::Method::Post,
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
        body: None,
    };
    let error = refresh_request(
        &super::super::Service::fixture(&endpoint),
        &request,
        &BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(!error.message.contains(&endpoint));
}

#[tokio::test]
async fn classifies_token_rejections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = rustls::crypto::ring::default_provider().install_default();
    for renewal in [false, true] {
        for (status, body, rejected) in [
            (401, "unauthorized", true),
            (400, r#"{"error":"invalid_grant"}"#, true),
            (400, r#"{"error":"temporarily_unavailable"}"#, false),
            (403, r#"{"error":"temporarily_unavailable"}"#, false),
            (503, r#"{"error":"temporarily_unavailable"}"#, false),
            (503, r#"{"error":"invalid_grant"}"#, false),
            (302, r#"{"error":"redirect"}"#, false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_buf(&mut bytes).await.unwrap();
                }
                stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: /next\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            });
            let endpoint = model_auth::Endpoint {
                cache_ttl_ms: None,
                url: "https://issuer.example.com/token".into(),
                method: model_auth::Method::Post,
                headers: BTreeMap::new(),
                query: BTreeMap::new(),
                body: None,
            };
            let service = super::super::Service::fixture(&origin);
            let stop = CancellationToken::new();
            let error = if renewal {
                refresh_request(&service, &endpoint, &BTreeMap::new(), &stop).await
            } else {
                request(&service, &endpoint, &BTreeMap::new(), &stop).await
            }
            .unwrap_err();
            assert_eq!(
                error.code == ErrorCode::PermissionDenied,
                rejected,
                "{renewal}: {status}"
            );
            server.await.unwrap();
        }
    }
}

#[test]
fn maps_public_account_labels() {
    let mut mapping = mapping();
    mapping.account_label = Some("/data/email".into());
    let value =
        json!({"data":{"access":"private-access","expires_in":60,"email":"display@example.test"}});
    assert_eq!(
        Issued::parse(&value, &mapping, 0, None)
            .unwrap()
            .account_label
            .as_deref(),
        Some("display@example.test")
    );
    assert_eq!(
        Issued::parse(
            &json!({"data":{"access":"private-access","expires_in":60}}),
            &mapping,
            0,
            None
        )
        .unwrap()
        .account_label,
        None
    );
}
