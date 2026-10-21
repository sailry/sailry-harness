use super::*;

#[tokio::test]
async fn isolates_concurrent_request_captures() {
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for token in ["first", "renewed"] {
        let observed = observed.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let scope = Scope {
                adapter: Arc::new(move |_, headers| {
                    headers.insert("authorization", HeaderValue::from_static(token));
                    Ok(())
                }),
                observer: Some(Arc::new(move |headers| {
                    observed
                        .lock()
                        .unwrap()
                        .push((token, headers["authorization"].to_str().unwrap().to_owned()));
                })),
            };
            REQUEST
                .scope(scope, async move {
                    barrier.wait().await;
                    let mut headers = HeaderMap::new();
                    adapter(&mut json!({}), &mut headers).unwrap();
                    tokio::task::yield_now().await;
                    observe(&headers);
                })
                .await;
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let mut observed = observed.lock().unwrap().clone();
    observed.sort();
    assert_eq!(
        observed,
        [
            ("first", "first".to_owned()),
            ("renewed", "renewed".to_owned())
        ]
    );
}

struct Lazy;
#[async_trait]
impl Llm for Lazy {
    fn name(&self) -> &str {
        "lazy"
    }
    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        Ok(Box::pin(futures::stream::try_unfold(
            (0, HeaderMap::new()),
            |(stage, mut headers)| async move {
                let response = match stage {
                    0 => {
                        adapter(&mut json!({}), &mut headers)?;
                        adk_core::LlmResponse {
                            partial: true,
                            ..Default::default()
                        }
                    }
                    1 => {
                        observe(&headers);
                        adk_core::LlmResponse {
                            turn_complete: true,
                            ..Default::default()
                        }
                    }
                    _ => return Ok::<_, AdkError>(None),
                };
                Ok(Some((response, (stage + 1, headers))))
            },
        )))
    }
}

#[tokio::test]
async fn retains_capture_while_streaming() {
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let scope = |token, captured_revision| {
        let observed = observed.clone();
        Scope {
            adapter: Arc::new(move |_, headers| {
                headers.insert("authorization", HeaderValue::from_static(token));
                Ok(())
            }),
            observer: Some(Arc::new(move |headers| {
                observed.lock().unwrap().push((
                    token,
                    headers["authorization"].to_str().unwrap().to_owned(),
                    captured_revision,
                ));
            })),
        }
    };
    let mut first = scoped(
        scope("old", 1),
        Arc::new(Lazy),
        LlmRequest::new("lazy", vec![]),
        true,
    )
    .await
    .unwrap();
    let mut renewed = scoped(
        scope("new", 2),
        Arc::new(Lazy),
        LlmRequest::new("lazy", vec![]),
        true,
    )
    .await
    .unwrap();
    assert!(renewed.next().await.unwrap().unwrap().partial);
    assert!(first.next().await.unwrap().unwrap().partial);
    assert!(first.next().await.unwrap().unwrap().turn_complete);
    assert!(renewed.next().await.unwrap().unwrap().turn_complete);
    assert_eq!(
        *observed.lock().unwrap(),
        [("old", "old".into(), 1), ("new", "new".into(), 2)]
    );
}

#[tokio::test]
async fn rotates_constructor_authorization() {
    use crate::providers::login::plugin::{Issued, Record, Target};
    use sailry_protocol::{
        Credential, CredentialId, CredentialRef, NodeId, ProviderId, Secret, SessionId,
        plugin::model_auth,
    };
    use std::collections::BTreeMap;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let records = observed.clone();
    let server = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        while let Ok((socket, _)) = listener.accept().await {
            let records = records.clone();
            tasks.spawn(async move {
                let service = hyper::service::service_fn(move |request:hyper::Request<hyper::body::Incoming>| {
                    records.lock().unwrap().push(request.headers()["x-goog-api-key"].to_str().unwrap().to_owned());
                    async move { Ok::<_,std::convert::Infallible>(hyper::Response::builder().header("content-type","application/json").body(http_body_util::Full::new(bytes::Bytes::from_static(br#"{"candidates":[{"content":{"role":"model","parts":[{"text":"answer"}]},"finishReason":"STOP","index":0}]}"#))).unwrap()) }
                });
                let _=hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(socket),service).await;
            });
        }
    });
    let id = ProviderId::new();
    let credential = CredentialId::new();
    let provider:Provider=serde_json::from_value(json!({"id":id,"revision":1,"name":"Native fixture","api":"gemini","authentication":"plugin","endpoint":endpoint,"enabled":true,"models":[],"default_model":"fixture"})).unwrap();
    let config:SessionConfig=serde_json::from_value(json!({"provider":id,"model":"fixture","mode":"code","permission":"ask","credential":CredentialRef{node:NodeId([4; 32]),id:credential}})).unwrap();
    let registry = registry::Registry::default();
    let session = SessionId::new();
    let mut previous = None;
    for (revision, key) in [
        (1, "old-private-key"),
        (1, "old-private-key"),
        (2, "new-private-key"),
    ] {
        let authorization = crate::store::login::Authorization {
            credential: Credential {
                id: credential,
                provider: id,
                authentication: Authentication::Plugin,
                account_label: None,
                revision,
                expires_at_ms: None,
                revoked: false,
            },
            grant: Grant::Plugin {
                record: Box::new(Record {
                    target: Target {
                        provider: id,
                        package: "fixture".into(),
                        digest: None,
                        origin: sailry_protocol::conversation::consent::Origin::Bundled,
                        flow: "fixture".into(),
                        endpoint: provider.endpoint.clone(),
                        authorization: model_auth::Flow::TokenExchange {
                            token: model_auth::Endpoint {
                                url: "https://auth.example.invalid/token".into(),
                                method: model_auth::Method::Post,
                                headers: BTreeMap::new(),
                                query: BTreeMap::new(),
                                body: None,
                                cache_ttl_ms: None,
                            },
                        },
                        refresh: None,
                        inject: vec![],
                        fields: BTreeMap::new(),
                        scopes: vec![],
                    },
                    issued: Issued {
                        account_label: None,
                        access: Secret::new(key.into()),
                        refresh: None,
                        expires_at_ms: None,
                        extra: BTreeMap::new(),
                    },
                }),
                renewal_pending: false,
                renewal_rejected: false,
            },
        };
        let entry = pooled(
            &registry,
            &provider,
            &config,
            session,
            reasoning::resolve(None, provider.api, Effort::Default).unwrap(),
            Purpose::Main,
            &authorization,
        )
        .unwrap();
        if let Some(previous) = previous {
            assert_eq!(Arc::ptr_eq(&entry, &previous), revision == 1);
        }
        let fingerprint =
            registry::fingerprint(&provider, &config, Effort::Default, Some(revision), false)
                .unwrap();
        assert!(!fingerprint.contains(key));
        let mut stream = super::super::errors::wrap(
            entry.inner.clone(),
            authorization.grant.secrets(&HeaderMap::new()),
        )
        .generate_content(
            LlmRequest::new(
                "fixture",
                vec![adk_core::Content::new("user").with_text("request")],
            ),
            false,
        )
        .await
        .unwrap();
        assert!(stream.next().await.unwrap().unwrap().turn_complete);
        previous = Some(entry);
    }
    assert_eq!(
        *observed.lock().unwrap(),
        ["old-private-key", "old-private-key", "new-private-key"]
    );
    server.abort();
    let _ = server.await;
}
