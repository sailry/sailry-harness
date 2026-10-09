use super::*;

#[tokio::test]
async fn captures_declared_prompts_from_each_hub() {
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        let mut catalog = Catalog::embedding();
        catalog.insert(
            "config_sentence_transformers.json",
            &json!({
                "prompts": {"query": "query: ", "passage": "passage: "},
                "default_prompt_name": null
            }),
        );
        let server = Server::start(move |request, _| match hub {
            Hub::HuggingFace => catalog.hf_reply(request),
            Hub::ModelScope => catalog.ms_reply(request),
        })
        .await;
        let definition = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(hub, REVISION),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(definition.query_prefix.as_deref(), Some("query: "));
        assert_eq!(definition.document_prefix.as_deref(), Some("passage: "));
        assert!(server.requests().iter().any(|request| {
            request.path.ends_with("config_sentence_transformers.json")
                || request
                    .query
                    .get("FilePath")
                    .is_some_and(|path| path == "config_sentence_transformers.json")
        }));
    }
}

#[tokio::test]
async fn honors_document_precedence_and_default_prompts() {
    for (prompts, default, query, document) in [
        (
            json!({"query":"q ","document":"d ","passage":"p ","corpus":"c "}),
            Value::Null,
            Some("q "),
            Some("d "),
        ),
        (
            json!({"query":"q "}),
            json!("query"),
            Some("q "),
            Some("q "),
        ),
        (json!({"corpus":"c "}), Value::Null, None, Some("c ")),
        (
            json!({"query":"", "document":""}),
            Value::Null,
            Some(""),
            Some(""),
        ),
    ] {
        let mut catalog = Catalog::embedding();
        catalog.insert(
            "config_sentence_transformers.json",
            &json!({"prompts":prompts,"default_prompt_name":default}),
        );
        let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
        let definition = resolve::definition(
            &server.http(),
            Kind::Embedding,
            &source(Hub::HuggingFace, REVISION),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(definition.query_prefix.as_deref(), query);
        assert_eq!(definition.document_prefix.as_deref(), document);
    }
}

#[tokio::test]
async fn rejects_unusable_declared_prompt_metadata() {
    for metadata in [
        json!({"prompts":{"query":true}}),
        json!({"prompts":{"query":"q\u{0000}"}}),
        json!({"prompts":{"query":"a".repeat(sailry_protocol::models::MAX_PREFIX_BYTES + 1)}}),
        json!({"prompts":[],"default_prompt_name":null}),
        json!({"prompts":{"query":"q "},"default_prompt_name":"missing"}),
        json!({"prompts":{"query":"q "},"default_prompt_name":"document"}),
    ] {
        let mut catalog = Catalog::embedding();
        catalog.insert("config_sentence_transformers.json", &metadata);
        let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
        assert_eq!(
            resolve::definition(
                &server.http(),
                Kind::Embedding,
                &source(Hub::HuggingFace, REVISION),
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(server.requests().len(), 2);
    }
}

#[tokio::test]
async fn rejects_excluding_declared_prompt_tokens() {
    let mut catalog = Catalog::embedding();
    catalog.insert(
        "config_sentence_transformers.json",
        &json!({"prompts":{"query":"query: "}}),
    );
    catalog.insert("1_Pooling/config.json", &json!({
        "word_embedding_dimension":384,"pooling_mode_cls_token":false,"pooling_mode_mean_tokens":true,"include_prompt":false
    }));
    let server = Server::start(move |request, _| catalog.hf_reply(request)).await;
    let error = resolve::definition(
        &server.http(),
        Kind::Embedding,
        &source(Hub::HuggingFace, REVISION),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("excluding prompt tokens"));
}
