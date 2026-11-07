use super::*;
use sailry_protocol::models::Pooling;

#[tokio::test]
async fn pins_normal_and_lfs_assets() {
    let catalog = Catalog::embedding();
    let config_hash = checksum(&catalog.files["config.json"].body);
    let tokenizer_hash = checksum(&catalog.files["tokenizer.json"].body);
    let graph_hash = checksum(&catalog.files["onnx/model.onnx"].body);
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "release/v1"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source.revision, REVISION);
    assert_eq!(
        definition.format,
        Format::Transformer {
            dimension: 384,
            pooling: Pooling::Cls,
            token_types: true
        }
    );
    assert_eq!(
        definition
            .assets
            .iter()
            .map(|asset| asset.name.as_str())
            .collect::<Vec<_>>(),
        vec!["config.json", "tokenizer.json", "model.onnx"]
    );
    assert_eq!(
        definition
            .assets
            .iter()
            .map(|asset| asset.sha256.as_str())
            .collect::<Vec<_>>(),
        vec![
            config_hash.as_str(),
            tokenizer_hash.as_str(),
            graph_hash.as_str()
        ]
    );
    assert!(
        definition
            .assets
            .iter()
            .all(|asset| asset.source.revision == REVISION && asset.sha256.len() == 64)
    );
    assert_eq!(definition.assets[2].path, "onnx/model.onnx");
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/api/models/org/model/revision/release%2Fv1"
    );
    assert_eq!(requests[0].query["blobs"], "true");
    assert_eq!(requests.len(), 4);
    assert!(
        !requests
            .iter()
            .any(|request| request.path.ends_with("model.onnx"))
    );
}

#[tokio::test]
async fn rejects_changed_git_blob_content() {
    let catalog = Catalog::embedding();
    let mut changed = catalog.files["config.json"].body.clone();
    let digit = changed.iter().position(|byte| *byte == b'4').unwrap();
    changed[digit] = b'5';
    let server = Server::start(move |request, _| {
        if request.path.ends_with("/config.json") {
            Reply::body(changed.clone())
        } else {
            catalog.hf_reply(request)
        }
    })
    .await;
    let error = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert!(error.message.contains("config.json"));
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn rejects_a_different_repository_commit() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let error = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, OLDER),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn captures_current_branch_file_revisions() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| catalog.ms_reply(request)).await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::ModelScope, "master"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source.revision, "master");
    assert_eq!(definition.assets[0].source.revision, OLDER);
    assert_eq!(definition.assets[1].source.revision, OLDER);
    assert_eq!(definition.assets[2].source.revision, NEWER);
    assert!(
        definition
            .assets
            .iter()
            .all(|asset| commit(&asset.source.revision) && digest(&asset.sha256))
    );
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .find(|request| request.path.ends_with("/repo/files"))
            .unwrap()
            .query["Revision"],
        "master"
    );
    for request in requests
        .iter()
        .filter(|request| request.path.ends_with("/repo"))
    {
        assert_eq!(request.query["Revision"], OLDER);
        assert!(matches!(
            request.query["FilePath"].as_str(),
            "config.json" | "tokenizer.json" | "1_Pooling/config.json"
        ));
    }
}

#[tokio::test]
async fn preserves_selected_historical_commits() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| {
        if request.path.ends_with("/repo/files") || request.path.ends_with("/repo") {
            assert_eq!(request.query["Revision"], REVISION);
        }
        catalog.ms_reply(request)
    })
    .await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::ModelScope, REVISION),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source.revision, REVISION);
    assert!(
        definition
            .assets
            .iter()
            .all(|asset| asset.source.revision == REVISION)
    );
    assert!(
        !server
            .requests()
            .iter()
            .any(|request| request.path.ends_with("/info/refs"))
    );
}

#[tokio::test]
async fn rejects_stale_selected_version_metadata() {
    for size_changed in [false, true] {
        let catalog = Catalog::embedding();
        let mut old_body = catalog.files["config.json"].body.clone();
        if size_changed {
            old_body.extend_from_slice(b" ");
        } else {
            let digit = old_body.iter().position(|byte| *byte == b'4').unwrap();
            old_body[digit] = b'5';
        }
        let server = Server::start(move |request, _| {
            if request.path.ends_with("/repo")
                && request.query.get("FilePath").map(String::as_str) == Some("config.json")
            {
                assert_eq!(request.query["Revision"], REVISION);
                Reply::body(old_body.clone())
            } else {
                catalog.ms_reply(request)
            }
        })
        .await;
        let error = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(Hub::ModelScope, REVISION),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert!(error.message.contains("config.json"));
        assert_eq!(server.requests().len(), 3);
        assert!(!server.requests().iter().any(|request| {
            request
                .query
                .get("Revision")
                .is_some_and(|revision| revision == "master" || revision == NEWER)
        }));
    }
}

#[tokio::test]
async fn fixes_peeled_tags_before_reading() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| {
        if request.path.ends_with("/info/refs") {
            return Reply::body(advertisement(&[
                ("refs/heads/master", REVISION),
                ("refs/tags/v1", NEWER),
                ("refs/tags/v1^{}", OLDER),
            ]));
        }
        if request.path.ends_with("/repo/files") || request.path.ends_with("/repo") {
            assert_eq!(request.query["Revision"], OLDER);
        }
        catalog.ms_reply(request)
    })
    .await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::ModelScope, "v1"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source.revision, OLDER);
    assert!(
        definition
            .assets
            .iter()
            .all(|asset| asset.source.revision == OLDER)
    );
    assert!(
        !server
            .requests()
            .iter()
            .any(|request| request.path.ends_with("/revisions"))
    );
}

#[tokio::test]
async fn refuses_ambiguous_latest_branch_selection() {
    for revision in ["master", "v1"] {
        let catalog = Catalog::embedding();
        let server = Server::start(move |request, _| {
            if request.path.ends_with("/revisions") {
                return Reply::json(json!({"Success":true,"Data":{"RevisionMap":{
                    "Branches":[{"Revision":"master"},{"Revision":"development"}],"Tags":[{"Revision":"v1"}]
                }}}));
            }
            catalog.ms_reply(request)
        }).await;
        let error = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(Hub::ModelScope, revision),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.path.ends_with("/repo/files"))
        );
    }
}

#[tokio::test]
async fn accepts_an_explicit_default_branch() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| {
        if request.path.starts_with("/openapi/v1/models/") {
            return Reply::json(json!({"success":true,"data":{"display_name":"Model","license":"mit","default_branch":"master"}}));
        }
        if request.path.ends_with("/revisions") {
            return Reply::json(json!({"Success":true,"Data":{"RevisionMap":{"Branches":[{"Revision":"master"},{"Revision":"development"}],"Tags":[]}}}));
        }
        catalog.ms_reply(request)
    }).await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::ModelScope, "master"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.source.revision, "master");
    assert!(
        definition
            .assets
            .iter()
            .all(|asset| commit(&asset.source.revision))
    );
}

#[tokio::test]
async fn derives_masked_mean_pooling() {
    let mut catalog = Catalog::embedding();
    catalog.insert(
        "config.json",
        &json!({"hidden_size":384,"model_type":"modernbert"}),
    );
    catalog.insert("2_Pooling/config.json", &json!({"word_embedding_dimension":384,"pooling_mode_cls_token":false,"pooling_mode_mean_tokens":true}));
    catalog.insert(
        "modules.json",
        &json!([
            {"type":"sentence_transformers.models.Transformer","path":""},
            {"type":"sentence_transformers.models.Pooling","path":"2_Pooling"},
            {"type":"sentence_transformers.models.Normalize","path":"3_Normalize"}
        ]),
    );
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        definition.format,
        Format::Transformer {
            dimension: 384,
            pooling: Pooling::Mean,
            token_types: false
        }
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.path.ends_with("2_Pooling/config.json"))
    );
}

#[tokio::test]
async fn rejects_unsupported_pooling_and_modules() {
    for modules in [false, true] {
        let mut catalog = Catalog::embedding();
        if modules {
            catalog.insert(
                "modules.json",
                &json!([{ "type":"remote_module.CustomDense","path":"custom" }]),
            );
        } else {
            catalog.insert("1_Pooling/config.json", &json!({"word_embedding_dimension":384,"pooling_mode_cls_token":true,"pooling_mode_mean_tokens":false,"pooling_mode_max_tokens":true}));
        }
        let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
        let error = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(Hub::HuggingFace, "main"),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("unsupported"));
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.path.ends_with("custom"))
        );
    }
}

#[tokio::test]
async fn retains_static_token_table_format() {
    let mut catalog = Catalog::embedding();
    catalog.insert(
        "config.json",
        &json!({"model_type":"model2vec","hidden_dim":256,"normalize":true,"sif_coefficient":0.0001}),
    );
    catalog.files.insert(
        "model.safetensors".into(),
        Data {
            body: b"resolver static table metadata fixture".to_vec(),
            lfs: true,
            revision: NEWER,
        },
    );
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let definition = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(definition.format, Format::Static { dimension: 256 });
    assert_eq!(definition.assets[2].name, "model.safetensors");
}

#[tokio::test]
async fn selects_int8_sense_voice_candidates() {
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        let catalog = Catalog::speech();
        let expected = checksum(&catalog.files["model.int8.onnx"].body);
        let server = Server::start(move |request, _| match hub {
            Hub::HuggingFace => catalog.hf_reply(request),
            Hub::ModelScope => catalog.ms_reply(request),
        })
        .await;
        let mut selected = source(
            hub,
            if hub == Hub::HuggingFace {
                "main"
            } else {
                "master"
            },
        );
        selected.repository = "org/sherpa-SenseVoice".into();
        let definition = resolve::definition(
            &server.http(),
            Kind::Speech,
            &selected,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(definition.format, Format::SenseVoice {});
        assert_eq!(definition.assets[0].name, "model.onnx");
        assert_eq!(definition.assets[0].path, "model.int8.onnx");
        assert_eq!(definition.assets[0].sha256, expected);
        assert_eq!(definition.assets[1].name, "tokens.txt");
        assert!(
            definition
                .assets
                .iter()
                .all(|asset| commit(&asset.source.revision) && digest(&asset.sha256))
        );
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.path.ends_with(".onnx"))
        );
    }
}

#[tokio::test]
async fn refuses_other_speech_algorithms() {
    let catalog = Catalog::speech();
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let error = resolve::definition(
        &server.http(),
        Kind::Speech,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("SenseVoice"));
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn rejects_escaping_sources_before_network() {
    let server = Server::start(|_, _| Reply::status(500)).await;
    let mut selected = source(Hub::HuggingFace, "main");
    selected.repository = "org/../another".into();
    assert_eq!(
        resolve::definition(
            &server.http(),
            Kind::Embedding,
            &selected,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn rejects_escaping_pooling_metadata_paths() {
    let mut catalog = Catalog::embedding();
    catalog.insert(
        "modules.json",
        &json!([
            {"type":"sentence_transformers.models.Transformer","path":""},
            {"type":"sentence_transformers.models.Pooling","path":"../outside"}
        ]),
    );
    catalog.insert("../outside/config.json", &json!({"word_embedding_dimension":384,"pooling_mode_cls_token":true,"pooling_mode_mean_tokens":false}));
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let error = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        !server
            .requests()
            .iter()
            .any(|request| request.path.contains("outside"))
    );
}

#[tokio::test]
async fn rejects_inconsistent_lfs_metadata_sizes() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |_, _| {
        let mut value = catalog.hf_info(REVISION);
        let graph = value["siblings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row.get("lfs").is_some())
            .unwrap();
        graph["lfs"]["size"] = json!(1);
        Reply::json(value)
    })
    .await;
    let error = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, "main"),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(server.requests().len(), 1);
}
