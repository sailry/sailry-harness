use super::*;

fn search(hub: Hub) -> Search {
    Search {
        hub,
        kind: Kind::Embedding,
        query: String::new(),
        cursor: None,
    }
}

#[tokio::test]
async fn skips_incompatible_pages_without_loading_assets() {
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        let valid = Catalog::embedding();
        let mut unsupported = Catalog::embedding();
        unsupported.files.remove("tokenizer.json");
        let server = Server::start(move |request, base| {
            let listing = if hub == Hub::HuggingFace {request.path == "/api/models"} else {request.path == "/openapi/v1/models"};
            if listing {
                let second = request.query.contains_key("cursor") || request.query.get("page_number").map(String::as_str) == Some("2");
                let repository = if second {"org/model"} else {"org/unsupported"};
                if hub == Hub::HuggingFace {
                    assert_eq!(request.query.get("filter").map(String::as_str), Some("onnx"));
                    assert!(!request.query.contains_key("search"));
                    let reply = Reply::json(json!([{"id":repository}]));
                    if second {return reply;}
                    return reply.header("Link", format!("<{}>; rel=\"next\"", base.join("api/models?cursor=second").unwrap()));
                }
                assert_eq!(request.query["search"], "onnx");
                return Reply::json(json!({"success":true,"data":{"models":[{"id":repository}],"total_count":21,"page_number":if second {2} else {1},"page_size":20}}));
            }
            if request.path.ends_with("/info/refs") {
                return Reply::body(advertisement(&[("refs/heads/master",REVISION)]));
            }
            let catalog = if request.path.contains("/unsupported/") || request.path.ends_with("/unsupported") {&unsupported} else {&valid};
            match hub {Hub::HuggingFace => catalog.hf_reply(request), Hub::ModelScope => catalog.ms_reply(request)}
        }).await;
        let page = compatible(&server.http(), &search(hub), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].source, source(hub, REVISION));
        assert_eq!(
            page.items[0].format,
            Format::Transformer {
                dimension: 384,
                pooling: sailry_protocol::models::Pooling::Cls,
                token_types: true
            }
        );
        assert!(page.next_cursor.is_none());
        assert!(!page.partial);
        assert!(
            server
                .requests()
                .iter()
                .all(|request| !request.path.ends_with("tokenizer.json")
                    && !request.path.ends_with(".onnx")
                    && !request.path.ends_with("tokens.txt")
                    && !matches!(
                        request.query.get("FilePath").map(String::as_str),
                        Some("tokenizer.json" | "tokens.txt" | "onnx/model.onnx")
                    ))
        );
    }
}

#[tokio::test]
async fn pins_default_branch_for_head() {
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        let catalog = Catalog::embedding();
        let server = Server::start(move |request, _| {
            if hub == Hub::ModelScope {
                if request.path.starts_with("/openapi/v1/models/") {
                    return Reply::json(json!({"success":true,"data":{"display_name":"Fixture model","license":"mit","default_branch":"master"}}));
                }
                if request.path.ends_with("/info/refs") {
                    return Reply::body(advertisement(&[("refs/heads/master", REVISION), ("refs/heads/development", NEWER)]));
                }
                if request.path.ends_with("/repo") || request.path.ends_with("/repo/files") {assert_eq!(request.query["Revision"], REVISION);}
                catalog.ms_reply(request)
            } else {
                if request.path.contains("/revision/") {assert!(request.path.ends_with("/revision/main"));}
                catalog.hf_reply(request)
            }
        }).await;
        let definition = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(hub, "HEAD"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(definition.source, source(hub, REVISION));
        assert!(
            definition
                .assets
                .iter()
                .all(|asset| asset.source.revision == REVISION && digest(&asset.sha256))
        );
    }
}

#[tokio::test]
async fn excludes_mutable_snapshots_without_rejecting_manual_resolution() {
    let catalog = Catalog::embedding();
    let expected = catalog
        .files
        .iter()
        .map(|(path, data)| {
            (
                path.clone(),
                (data.revision, checksum(&data.body), data.body.len() as u64),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let server = Server::start(move |request, _| {
        if request.path == "/openapi/v1/models" {
            return Reply::json(json!({
                "success":true,
                "data":{
                    "models":[{"id":"org/model"}],
                    "total_count":1,
                    "page_number":1,
                    "page_size":20
                }
            }));
        }
        catalog.ms_reply(request)
    })
    .await;
    let page = compatible(
        &server.http(),
        &search(Hub::ModelScope),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(page.items.is_empty());
    assert!(page.partial);
    assert!(page.next_cursor.is_none());
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.path.ends_with("/info/refs"))
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.ends_with("/repo"))
    );

    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::ModelScope, "HEAD"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source, source(Hub::ModelScope, "master"));
    assert_eq!(definition.assets.len(), 3);
    for asset in &definition.assets {
        let (revision, sha256, bytes) = &expected[&asset.path];
        assert_eq!(asset.source, source(Hub::ModelScope, revision));
        assert_eq!(&asset.sha256, sha256);
        assert_eq!(asset.bytes, *bytes);
    }
    definition.validate().unwrap();
}

#[tokio::test]
async fn distinguishes_unsupported_formats_from_service_failures() {
    let mut catalog = Catalog::embedding();
    catalog.insert("1_Pooling/config.json", &json!({"word_embedding_dimension":384,"pooling_mode_cls_token":true,"pooling_mode_max_tokens":true}));
    let server = Server::start(move |request, _| {
        if request.path == "/api/models" {
            return Reply::json(json!([{"id":"org/model"},{"id":"org/unavailable"}]));
        }
        if request.path.contains("/unavailable/") {
            return Reply::status(503);
        }
        catalog.hf_reply(request)
    })
    .await;
    let page = compatible(
        &server.http(),
        &search(Hub::HuggingFace),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(page.items.is_empty());
    assert!(page.partial);
    assert!(page.next_cursor.is_none());
    assert!(server.requests().iter().all(
        |request| !request.path.ends_with("tokenizer.json") && !request.path.ends_with(".onnx")
    ));
}

#[tokio::test]
async fn refuses_nonadvancing_cursor() {
    let server = Server::start(|_, base| {
        Reply::json(json!([])).header(
            "Link",
            format!(
                "<{}>; rel=\"next\"",
                base.join("api/models?cursor=same").unwrap()
            ),
        )
    })
    .await;
    let mut request = search(Hub::HuggingFace);
    request.cursor = Some("same".into());
    assert_eq!(
        compatible(&server.http(), &request, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn lists_supported_static_and_speech_shapes() {
    for kind in [Kind::Embedding, Kind::Speech] {
        let mut catalog = if kind == Kind::Embedding {
            Catalog::embedding()
        } else {
            Catalog::speech()
        };
        if kind == Kind::Embedding {
            catalog.insert(
                "config.json",
                &json!({"model_type":"model2vec","hidden_dim":256,"normalize":true}),
            );
            catalog.files.insert(
                "model.safetensors".into(),
                Data {
                    body: b"static table fixture".to_vec(),
                    lfs: true,
                    revision: REVISION,
                },
            );
        }
        let server = Server::start(move |request, _| {
            if request.path == "/api/models" {
                return Reply::json(json!([{"id":"org/sensevoice"}]));
            }
            catalog.hf_reply(request)
        })
        .await;
        let mut request = search(Hub::HuggingFace);
        request.kind = kind;
        let page = compatible(&server.http(), &request, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            page.items[0].format,
            if kind == Kind::Embedding {
                Format::Static { dimension: 256 }
            } else {
                Format::SenseVoice {}
            }
        );
        assert!(
            server
                .requests()
                .iter()
                .all(|request| !request.path.ends_with("tokenizer.json")
                    && !request.path.ends_with(".onnx")
                    && !request.path.ends_with(".safetensors")
                    && !request.path.ends_with("tokens.txt"))
        );
    }
}
